{
  description = "nexus-rs addon fo yo hole";
  inputs = {
    flake-compat = {
      url = "github:edolstra/flake-compat";
      flake = false;
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, fenix, flake-utils, crane, nixpkgs, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
          crossSystem.config = "x86_64-w64-mingw32";
        };

        packageToolchain = with fenix.packages.${system};
          combine [
            minimal.rustc
            minimal.cargo
            targets.x86_64-pc-windows-gnu.latest.rust-std
          ];

        packageCraneLib = (crane.mkLib pkgs).overrideToolchain (p: packageToolchain);

        example = pkgs.callPackage ./package.nix {
          craneLib = packageCraneLib;
        };

        shellToolchain = with fenix.packages.${system};
          combine [
            complete
            rust-analyzer
            targets.x86_64-pc-windows-gnu.latest.rust-std
          ];

        shellCraneLib = (crane.mkLib pkgs).overrideToolchain (p: shellToolchain);

        exampleShell = import ./shell.nix {
          inherit fenix pkgs system;
        };
      in
      rec {
        defaultPackage = packages.x86_64-pc-windows-gnu;
        inherit pkgs;
        devShells.default = exampleShell;

        packages = {
          inherit example;
          default = example;
        };
      });
}

