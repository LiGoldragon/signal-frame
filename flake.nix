{
  description = "Signal frame mechanics — envelope, length-prefixed rkyv archives, handshake, exchange identifiers, async correlation, streams, reply plumbing";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, fenix, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        toolchain = fenix.packages.${system}.fromToolchainFile {
          file = ./rust-toolchain.toml;
          sha256 = "sha256-gh/xTkxKHL4eiRXzWv8KP7vfjSk61Iq48x47BEDFgfk=";
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (builtins.match ".*\\.schema$" path != null)
            || (builtins.match ".*\\.stderr$" path != null)
            || (builtins.match ".*/tests/golden/.*\\.bin$" path != null);
        };
        commonArgs = {
          inherit src;
          strictDeps = true;
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      in
      {
        packages.default = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        });

        checks = {
          default = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });
          test-nota-text = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
            cargoTestExtraArgs = "--all-targets --features nota-text";
          });
          fmt = craneLib.cargoFmt {
            inherit src;
          };
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -D warnings";
          });
          clippy-nota-text = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets --features nota-text -- -D warnings";
          });

          old-schema-composer-removed = pkgs.runCommand
            "old-schema-composer-removed"
            { }
            ''
              if test -d ${src}/schema-rust || \
                 ${pkgs.ripgrep}/bin/rg -n "emit_schema|schema_rust|^schema-rust[[:space:]]*=|^schema[[:space:]]*=" \
                   ${src}/Cargo.toml ${src}/macros/Cargo.toml ${src}/src ${src}/macros/src
              then
                echo "signal-frame must not carry the retired local schema-rust / emit_schema path" >&2
                exit 1
              fi
              touch $out
            '';
        };

        devShells.default = pkgs.mkShell {
          name = "signal-frame";
          packages = [
            pkgs.jujutsu
            pkgs.pkg-config
            toolchain
          ];
        };
      }
    );
}
