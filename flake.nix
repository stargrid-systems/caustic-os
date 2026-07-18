{
  description = "Caustic OS - NixOS-based embedded OS for the eCube energy storage system";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    nixos-hardware.url = "github:NixOS/nixos-hardware";

    pre-commit-hooks-nix = {
      url = "github:cachix/pre-commit-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane = {
      url = "github:ipetkov/crane/v0.23.4";
    };

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # TODO: replace with Aperture's own flake once upstream adds one.
    aperture-src = {
      url = "github:stargrid-systems/aperture/24071a1859211d04dff6e0f20c393a9f2a68f7ba";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      pre-commit-hooks-nix,
      treefmt-nix,
      crane,
      rust-overlay,
      aperture-src,
      nixos-hardware,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      perSystem = nixpkgs.lib.genAttrs supportedSystems;

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

      treefmtModule = {
        projectRootFile = "flake.nix";
        programs.nixfmt.enable = true;
      };
      treefmtEvalFor = system: treefmt-nix.lib.evalModule (pkgsFor system) treefmtModule;

      preCommitHooksFor =
        system:
        let
          treefmtEval = treefmtEvalFor system;
        in
        pre-commit-hooks-nix.lib.${system}.run {
          src = ./.;
          settings.treefmt.package = treefmtEval.config.build.wrapper;
          hooks = {
            statix.enable = true;
            deadnix.enable = true;
            treefmt.enable = true;
          };
        };

      # Build aperture from source using the toolchain pinned in its rust-toolchain.toml.
      # This is a temporary local package; once aperture exposes its own flake we will
      # consume that directly and remove this code path.
      craneLibFor =
        system:
        let
          pkgs = pkgsFor system;
        in
        (crane.mkLib pkgs).overrideToolchain (
          p: p.rust-bin.fromRustupToolchainFile "${aperture-src}/rust-toolchain.toml"
        );

      apertureFor =
        system:
        let
          craneLib = craneLibFor system;
          crateName = craneLib.crateNameFromCargoToml {
            cargoToml = "${aperture-src}/aperture/Cargo.toml";
          };
        in
        craneLib.buildPackage {
          src = aperture-src;
          inherit (crateName) pname version;
          strictDeps = true;
          doCheck = false;
        };

      # Expose aperture as an attribute on nixpkgs so the service module can
      # use `mkPackageOption pkgs "aperture" { ... }` without specialArgs.
      apertureOverlay = final: _prev: {
        aperture = apertureFor final.stdenv.hostPlatform.system;
      };

      # A NixOS configuration that boots as a QEMU VM on x86_64-linux.
      devNixosFor =
        system:
        nixpkgs.lib.nixosSystem {
          inherit system;
          modules = [
            { nixpkgs.overlays = [ apertureOverlay ]; }
            self.nixosModules.aperture
            self.nixosModules.dropbear
            ./systems/dev/default.nix
          ];
        };
    in
    {
      overlays.default = apertureOverlay;

      nixosModules = {
        cm4PoeUps = import ./hardware/cm4-poe-ups { inherit nixos-hardware; };
        aperture = import ./modules/services/aperture.nix;
        dropbear = import ./modules/services/dropbear.nix;
      };

      nixosConfigurations.dev = devNixosFor "x86_64-linux";

      packages = perSystem (
        system:
        {
          aperture = apertureFor system;
          default = apertureFor system;
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          # Convenience: build the dev VM directly without going through
          # `nixosConfigurations.dev.config.system.build.vm`.
          devVm = self.nixosConfigurations.dev.config.system.build.vm;
        }
      );

      checks = perSystem (system: {
        formatting = (treefmtEvalFor system).config.build.check self;
        pre-commit = preCommitHooksFor system;
      });

      devShells = perSystem (
        system:
        let
          pkgs = pkgsFor system;
          treefmt = (treefmtEvalFor system).config.build.wrapper;
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.nil
              pkgs.statix
              pkgs.deadnix
              treefmt
            ];
            inherit (preCommitHooksFor system) shellHook;
          };
        }
      );

      formatter = perSystem (system: (treefmtEvalFor system).config.build.wrapper);
    };
}
