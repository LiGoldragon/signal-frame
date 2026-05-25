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

          schema-macro-does-not-use-legacy-emitter = pkgs.runCommand
            "schema-macro-does-not-use-legacy-emitter"
            { }
            ''
              if ${pkgs.ripgrep}/bin/rg -n \
                "ChannelSpec|schema_reader|emit::emit|crate::emit|crate::model" \
                ${src}/schema-rust/src ${src}/macros/src/schema_entry.rs
              then
                echo "emit_schema/schema-rust path must not use the legacy ChannelSpec emitter" >&2
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
