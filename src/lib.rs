use nexus::{
    event::{arc::{CombatData, ACCOUNT_NAME, COMBAT_LOCAL}, event_subscribe, Event},
    event_consume,
    gui::{register_render, render, RenderType},
    imgui::{sys::cty::c_char, Window},
    keybind::{keybind_handler, register_keybind_with_string},
    paths::get_addon_dir,
    quick_access::add_quick_access,
    texture::{load_texture_from_file, texture_receive, Texture},
    AddonFlags, UpdateProvider,
};
use buttplug::{
  client::{device::ScalarValueCommand, ButtplugClientDevice, ButtplugClient, ButtplugClientError},
  core::{
    connector::{
      new_json_ws_client_connector,
    },
    message::{ClientGenericDeviceMessageAttributesV3},
  },
};
use tokio::{runtime, select, task::JoinSet};
use tokio::sync::mpsc::{Receiver, Sender, channel, error::TryRecvError};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use std::{cell::{Cell, Ref, RefCell}, collections::VecDeque, ffi::CStr, ptr, thread::{self, JoinHandle}};
use std::sync::OnceLock;
use std::sync::Once;
use arcdps::{evtc::event::{EnterCombatEvent, Event as arcEvent}, Agent, AgentOwned};
use arcdps::Affinity;
use std::sync::Arc;

static SENDER: OnceLock<Sender<ButtplugThreadEvent>> = OnceLock::new();
static BP_THREAD: OnceLock<JoinHandle<()>> = OnceLock::new();

nexus::export! {
    name: "gw2buttplug-rs",
    signature: -0x7331BABE, // raidcore addon id or NEGATIVE random unique signature
    load,
    unload,
    flags: AddonFlags::None,
    provider: UpdateProvider::GitHub,
    update_link: "https://github.com/kittywitch/gw2buttplug-rs",
    log_filter: "debug"
}

enum ButtplugThreadEvent {
    Connect(String),
    Disconnect,
    Quit,
    SetGoodDps(i64),
    CombatEvent{src: arcdps::AgentOwned, evt: arcEvent},
}

#[derive(Eq, PartialEq, Debug)]
struct DamageEvent {
    time: i64,
    damage: i64,
}

struct ButtplugState {
    agent: Option<AgentOwned>,
    average: i64,
    intensity: f64,
    good_dps: i64,
    combat_scenario: VecDeque<DamageEvent>,
    bp_client: ButtplugClient,
}

impl ButtplugState {
    fn calculate_dps(&mut self) -> i64 {
        let oldest = self.combat_scenario.front();
        let latest = self.combat_scenario.back();
        let duration = match (oldest, latest) {
            (Some(oldest), Some(latest)) => latest.time - oldest.time,
            _ => {
                return 0
            },
        };
        let damage_sum = self.combat_scenario.iter().map(|e| e.damage).sum();
        match duration/1000 {
            0 => damage_sum,
            duration_s => damage_sum / duration_s,
        }
    }

    async fn tick(&mut self) -> anyhow::Result<()> {
        if !self.combat_scenario.is_empty() {
            // calculate average
            self.average = self.calculate_dps(); 
            self.intensity = self.average as f64 / self.good_dps as f64;
            let intensity_scalar = ScalarValueCommand::ScalarValue(self.intensity);
            let mut js: JoinSet<_> = self.bp_client.devices().into_iter().map(|device| device.vibrate(&intensity_scalar)).collect();
            while let Some(res) = js.join_next().await {
                res??;
            }
            // calculate dps for testing
            log::info!("DPS for the latest minute: {}", self.average);
            log::info!("Intensity: {}", self.intensity);
        }
        Ok(())
    }

    async fn handle_event(&mut self, event: ButtplugThreadEvent) -> anyhow::Result<bool> {
        use ButtplugThreadEvent::*;
        match event {
            Connect(address) => {
                let connector = new_json_ws_client_connector(&address);
                log::info!("Connecting to {}", address);
                self.bp_client.connect(connector).await?;
            },
            Disconnect => {
                self.bp_client.disconnect().await?;
                log::info!("Disconnecting");
            },
            Quit => {
                if let Err(error) = self.bp_client.disconnect().await {
                    log::error!("{}", error);
                }
                return Ok(false)
            },
            SetGoodDps(dps) => {
               self.good_dps = dps; 
            },
            CombatEvent { src, evt } => {
                use arcdps::StateChange;
                let is_self = src.is_self != 0;
                if is_self {
                    match &mut self.agent {
                        Some(agent) if src.name != agent.name => {
                            log::info!("Character changed from {:?} to {:?}!", agent.name, src.name);
                            *agent = src; 
                        },
                        Some(agent) => (),
                        None => {
                            log::info!("Character selected, {:?}!", src.name);
                            self.agent = Some(src);
                        }
                    }
                }
                match evt.get_statechange() {
                    StateChange::None => {
                        if is_self && evt.get_affinity() == Affinity::Foe {
                            let event_time = evt.time as i64;
                            log::info!("You did {} damage at {}!", -evt.value, evt.time);
                            // check for all items prior to a minute ago
                            let rem_partition_idx = self.combat_scenario.partition_point(|e| e.time <= event_time - (3600*1000));
                            self.combat_scenario = self.combat_scenario.split_off(rem_partition_idx);
                            // adding an item
                            let add_partition_idx = self.combat_scenario.partition_point(|e| e.time <= event_time);
                            self.combat_scenario.insert(add_partition_idx, DamageEvent { time: event_time, damage: -evt.value as i64});
                        }
                    },
                    StateChange::EnterCombat => {
                        log::info!("Combat begins at {}!", evt.time);
                    },
                    StateChange::ExitCombat => {
                        log::info!("Combat ends at {}!", evt.time);
                        self.combat_scenario.clear();
                        self.intensity = 0.0;
                        self.average = 0;
                        let intensity_scalar = ScalarValueCommand::ScalarValue(0.0);
                        let mut js: JoinSet<_> = self.bp_client.devices().into_iter().map(|device| device.vibrate(&intensity_scalar)).collect();
                        while let Some(res) = js.join_next().await {
                            res??;
                        }
                    },
                    _  => (),
                }
            }
        }
        Ok(true)
    }
}

fn load_buttplug(mut bp_receiver: Receiver<ButtplugThreadEvent>) {
    let bp_client = ButtplugClient::new("GW2Buttplug-rs");
    let mut state = ButtplugState {
        agent: None,
        average: 0,
        intensity: 0.0,
        good_dps: 10000,
        combat_scenario: VecDeque::new(),
        bp_client: bp_client,
    };
    let evt_loop = async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(250));
        loop {
            select! {
                evt = bp_receiver.recv() => match evt {
                    Some(evt) => {
                        match state.handle_event(evt).await {
                            Ok(true) => (),
                            Ok(false) => break,
                            Err(error) => {
                                log::error!("Error! {}", error)
                            }
                        } 
                    },
                    None => {
                        // presumably if the application is quitting, we want to shut off the buttplug client connection
                        // we unwrap because we don't really care what happens there
                        state.bp_client.disconnect().await.unwrap();
                        // break the event loop so the thread can exit
                        break
                    },
                },
                _ = interval.tick() => {
                    // does stuff every second
                    state.tick().await;
                },
            }
        }
    };
    let rt = match runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(error) => {
            log::error!("Error! {}", error);
            return
        },
        
    };
    rt.block_on(evt_loop);
}

fn load() {
    log::info!("Loading addon");
    let intiface_server_default = "ws://localhost:12345";
    let addon_dir = get_addon_dir("buttplug").expect("invalid addon dir");
    let (event_sender, event_receiver) = channel::<ButtplugThreadEvent>(32);
    let bp_handler = thread::spawn(|| { load_buttplug(event_receiver) });
    BP_THREAD.set(bp_handler);
    SENDER.set(event_sender);

    register_render(
        RenderType::Render,
        render!(|ui| {
            Window::new("Buttplug Control").build(ui, || {
                static START: Once = Once::new();
                // this is fine since imgui is single threaded
                thread_local! {
                    static CONNECT: Cell<bool> = const { Cell::new(false) };
                    static INTIFACE_ADDRESS: RefCell<String> = const { RefCell::new(String::new())};
                    static GOOD_DPS: RefCell<i32> = const { RefCell::new(10000i32) };
                }

                START.call_once(|| {
                    INTIFACE_ADDRESS.with_borrow_mut(|address|
                        if address.is_empty() {
                            *address = "ws://localhost:12345".to_string();
                        }
                    );
                });
                
                INTIFACE_ADDRESS.with_borrow_mut(|address| { 
                    let intiface_address = ui.input_text("Intiface Address", address).build();
                });

                let connect = CONNECT.get();
                if connect {
                    let sender = SENDER.get().unwrap();
                    if ui.button("Disconnect") {
                        sender.try_send(ButtplugThreadEvent::Disconnect);
                    }

                } else {
                    if ui.button("Connect") {
                        let intiface_address_final = INTIFACE_ADDRESS.with_borrow(|address| {
                            address.clone()
                        });
                        let sender = SENDER.get().unwrap();
                        sender.try_send(ButtplugThreadEvent::Connect(intiface_address_final));
                    }
                }

                GOOD_DPS.with_borrow_mut(|dps| {
                    let good_dps = ui.input_int("Good DPS", dps).build();
                });
                if ui.button("Save DPS") {
                    let good_dps_final = GOOD_DPS.with_borrow(|dps| {
                        dps.clone()
                    });
                    let sender = SENDER.get().unwrap();
                    sender.try_send(ButtplugThreadEvent::SetGoodDps(good_dps_final as i64));
                }

            });
        }),
    )
    .revert_on_unload();

    let combat_callback = event_consume!(|cdata: Option<&CombatData>| {
        let sender = SENDER.get().unwrap();
        if let Some(combat_data) = cdata {
            if let Some(evt) = combat_data.event() {
                if let Some(agt) = combat_data.src() {
                    let agt = AgentOwned::from(unsafe { ptr::read(agt) });
                    sender.try_send(ButtplugThreadEvent::CombatEvent {src: agt, evt: evt.clone()});
                }
            }
        }
    });

    COMBAT_LOCAL.subscribe(combat_callback).revert_on_unload();

    let receive_texture =
        texture_receive!(|id: &str, _texture: Option<&Texture>| log::info!("texture {id} loaded"));
    load_texture_from_file("BUTTTPLUG_ICON", addon_dir.join("icon.png"), Some(receive_texture));
    load_texture_from_file(
        "BUTTTPLUG_ICON_HOVER",
        addon_dir.join("icon_hover.png"),
        Some(receive_texture),
    );

    add_quick_access(
        "Buttplug Control",
        "BUTTTPLUG_ICON",
        "BUTTTPLUG_ICON_HOVER",
        "BUTTPLUG_MENU_KEYBIND",
        "Open buttplug control menu",
    )
    .revert_on_unload();
    
    let keybind_handler = keybind_handler!(|id, is_release| log::info!(
        "Keybind {id} {}",
        if is_release { "released" } else { "pressed" }
    ));
    register_keybind_with_string("BUTTPLUG_MENU_KEYBIND", keybind_handler, "ALT+SHIFT+T").revert_on_unload();

    unsafe { event_subscribe!("MY_EVENT" => i32, |data| log::info!("Received event {data:?}")) }
        .revert_on_unload();

    ACCOUNT_NAME
        .subscribe(event_consume!(<c_char> |name| {
            if let Some(name) = name {
                let name = unsafe {CStr::from_ptr(name as *const c_char)};
                log::info!("Received account name: {name:?}");
            }
        }))
        .revert_on_unload();
}

fn unload() {
    log::info!("Unloading addon");
    // all actions passed to on_load() or revert_on_unload() are performed automatically
    let sender = SENDER.get().unwrap();
    sender.try_send(ButtplugThreadEvent::Quit);
}