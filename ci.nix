{ config, pkgs, lib, ... }: with pkgs; with lib; let
  self = import ./.;
  packages = self.packages.${pkgs.system};
  artifactRoot = ".ci/artifacts";
  artifacts = "${artifactRoot}/lib/*.dll";
  release = "${artifactRoot}/lib/gw2buttplug-rs.dll";
in
{
  config = {
    name = "gw2buttplug-rs";
    ci.gh-actions = {
      enable = true;
      export = true;
    };
    # TODO: add cachix
    cache.cachix.example = {
      enable = true;
    };
    channels = {
      nixpkgs = {
        # see https://github.com/arcnmx/nixexprs-rust/issues/10
        args.config.checkMetaRecursively = false;
        version = "22.11";
      };
    };
    tasks = {
      build.inputs = with packages; [ example ];
      cache.inputs = with packages; [ example example.cargoArtifacts ];
    };
    jobs = {
      main = {
        tasks = {
          build-windows.inputs = singleton packages.example;
          build-windows-space.inputs = singleton packages.example;
        };
        artifactPackages = {
          main = packages.example;
        };
      };
    };

    # XXX: symlinks are not followed, see https://github.com/softprops/action-gh-release/issues/182
    #artifactPackage = config.artifactPackages.win64;
    artifactPackage = runCommand "example-artifacts" { } (''
      mkdir -p $out/lib
      cp ${config.artifactPackages.main}/lib/gw2buttplug_rs.dll $out/lib/
    '' + concatStringsSep "\n" (mapAttrsToList (key: addonPath: ''
        cp ${addonPath}/lib/gw2buttplug_rs.dll $out/lib/gw2buttplug_rs-${key}.dll
    '') config.artifactPackages));

    gh-actions = {
      jobs = mkIf (config.id != "ci") {
        ${config.id} = {
          permissions = {
            contents = "write";
          };
          step = {
            artifact-build = {
              order = 1100;
              name = "artifact build";
              uses = {
                # XXX: a very hacky way of getting the runner
                inherit (config.gh-actions.jobs.${config.id}.step.ci-setup.uses) owner repo version;
                path = "actions/nix/build";
              };
              "with" = {
                file = "<ci>";
                attrs = "config.jobs.${config.jobId}.artifactPackage";
                out-link = artifactRoot;
              };
            };
            artifact-upload = {
              order = 1110;
              name = "artifact upload";
              uses.path = "actions/upload-artifact@v4";
              "with" = {
                name = "gw2buttplug-rs";
                path = artifacts;
              };
            };
            release-upload = {
              order = 1111;
              name = "release";
              "if" = "startsWith(github.ref, 'refs/tags/')";
              uses.path = "softprops/action-gh-release@v1";
              "with".files = release;
            };
          };
        };
      };
    };
  };
  options = {
    artifactPackage = mkOption {
      type = types.package;
    };
    artifactPackages = mkOption {
      type = with types; attrsOf package;
    };
  };
}

