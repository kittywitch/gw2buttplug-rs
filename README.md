# GW2Buttplug-rs

A [Nexus](https://raidcore.gg/Nexus) mod for Guild Wars 2 that provides ArcDPS-derived buttplug.io control.

Currently provided at the minimum-viable proof of concept!

## To-dos

* Test support under Wine
* Implement better state machine for vibration control
    * Consider fade out mechanics better, instead of immediate shut-off
    * Consider how to handle fading between combat and out-of-combat repeatedly in quick succession
        * Intensity hold-over for a period?
* Communicate connection status with imgui thread
* Bind window control to a quickbar icon, allow toggling the window
* Add curve GUI for intensity control
* Fix minions/mech/... being included as damage sources for the player
    * See src_master_instance_id, https://zerthox.github.io/arcdps-rs/arcdps/struct.Event.html#structfield.src_master_instance_id
* Add boonDPS, boonHeal, pureDPS, masochist, harvestslut presets
    * selection of Q/A boon uptime % (0-1) * dps for BoonDPS
    * selection of Q/A boon uptime % (0-1) * currentHeal/goodHeal * currentDPS/goodDPS for BoonHeal
    * pureDPS is self-explanatory, already implemented
    * masochist gets intensity from higher damage incoming per second
    * harvest-related intensity might not be possible; no combat event associated with harvesting
* Add specialization & character detection, profiles...
* Add quick bar selections on a per-character basis for presets/profiles
* Add support for controlling separate devices using different profile/preset/curve/decision-making
* Clean up
* License