{
  description = "Caustic OS - NixOS-based embedded OS for the eCube energy storage system";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    nixos-hardware = {
      url = "github:NixOS/nixos-hardware";
      inputs.nixpkgs.follows = "nixpkgs";
    };

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

    aperture-src = {
      url = "github:stargrid-systems/aperture/67074b340519002b7fee12520a35d1538e56d913";
      flake = false;
    };

    impermanence = {
      url = "github:nix-community/impermanence";
      inputs.nixpkgs.follows = "nixpkgs";
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
      impermanence,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      perSystem = nixpkgs.lib.genAttrs supportedSystems;
      inherit (nixpkgs) lib;

      baseOverlays = [
        rust-overlay.overlays.default
        (_final: prev: {
          vhost-device-vsock = prev.vhost-device-vsock.overrideAttrs (_old: {
            doCheck = false;
          });
        })
      ];

      autoPatchelfOverlay = _final: prev: {
        "auto-patchelf" = prev."auto-patchelf".overrideAttrs (old: {
          postInstall = (old.postInstall or "") + ''
            sed -i '1 a import sys; sys.path.insert(0, "${prev.python3Packages.pyelftools}/lib/python${prev.python3.pythonVersion}/site-packages")' \
              $out/bin/auto-patchelf
          '';
        });
      };

      fetchCargoVendorOverlay = _final: prev: {
        fetch-cargo-vendor-util = prev.runCommand "fetch-cargo-vendor-util"
          { nativeBuildInputs = [ prev.makeWrapper ]; }
          ''
            mkdir -p $out/bin
            makeWrapper ${prev.fetch-cargo-vendor-util}/bin/fetch-cargo-vendor-util \
              $out/bin/fetch-cargo-vendor-util \
              --prefix PYTHONPATH : ${prev.python3Packages.requests}/${prev.python3.sitePackages}
          '';
      };

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = baseOverlays
            ++ lib.optional (system == "aarch64-linux") autoPatchelfOverlay
            ++ lib.optional (system == "aarch64-linux") fetchCargoVendorOverlay;
        };

      systemPkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = baseOverlays ++ lib.optional (system == "aarch64-linux") fetchCargoVendorOverlay;
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
          hooks.treefmt.package = treefmtEval.config.build.wrapper;
          hooks = {
            statix.enable = true;
            deadnix.enable = true;
            treefmt.enable = true;
          };
        };

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
          crateName = craneLib.crateNameFromCargoToml { src = aperture-src; };
        in
        craneLib.buildPackage {
          src = aperture-src;
          pname = "aperture";
          inherit (crateName) version;
          strictDeps = true;
          doCheck = false;
          auditable = false;
        };

      causticOtaFor =
        system:
        let
          craneLib = craneLibFor system;
          crateName = craneLib.crateNameFromCargoToml {
            cargoToml = ./crates/caustic-ota/Cargo.toml;
          };
          crate = craneLib.buildPackage {
            src = craneLib.cleanCargoSource ./.;
            inherit (crateName) pname version;
            cargoExtraArgs = "--package caustic-ota";
            strictDeps = true;
            doCheck = false;
          };
        in
        crate.overrideAttrs (old: {
          meta = (old.meta or { }) // {
            mainProgram = "caustic-ota";
          };
        });

      osOverlay = final: _prev: {
        aperture = apertureFor final.stdenv.hostPlatform.system;
        caustic-ota = causticOtaFor final.stdenv.hostPlatform.system;
      };

      devNixosFor =
        system:
        nixpkgs.lib.nixosSystem {
          inherit system;
          modules = [
            { nixpkgs.overlays = [ osOverlay ]; }
            self.nixosModules.aperture
            self.nixosModules.dropbear
            ./systems/dev/default.nix
          ];
        };

      prodNixosFor =
        system:
        {
          imageId ? "caustic-os",
          otaRegistry ? "ghcr.io/stargrid-systems/caustic-os",
          extraModules ? [ ],
        }:
        nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = {
            inherit imageId otaRegistry;
          };
          modules = [
            { nixpkgs.overlays = [ osOverlay ]; }
            self.nixosModules.kernel
            self.nixosModules.nativeBoot
            self.nixosModules.cm4PoeUps
            self.nixosModules.aperture
            self.nixosModules.dropbear
            self.nixosModules.caustic
            self.nixosModules.causticOta
            self.nixosModules.persist
            ./systems/production/default.nix
          ]
          ++ extraModules;
        };
    in
    {
      overlays.default = osOverlay;

      nixosModules = {
        cm4PoeUps = import ./hardware/cm4-poe-ups;
        aperture = import ./modules/services/aperture.nix;
        dropbear = import ./modules/services/dropbear.nix;
        caustic = import ./modules/caustic;
        causticOta = import ./modules/services/caustic-ota.nix;
        persist = import ./modules/persist { inherit impermanence; };
        nativeBoot = import ./modules/boot/default.nix;
        kernel = import ./modules/kernel/default.nix;
        dev-access = import ./modules/dev-access.nix;
      };

      nixosConfigurations = {
        devVm = devNixosFor "x86_64-linux";
        production = prodNixosFor "aarch64-linux" { };
        devImage = prodNixosFor "aarch64-linux" {
          imageId = "caustic-os-dev";
          otaRegistry = "ghcr.io/stargrid-systems/caustic-os-dev";
          extraModules = [ self.nixosModules.dev-access ];
        };
      };

      packages = perSystem (
        system:
        {
          aperture = apertureFor system;
          caustic-ota = causticOtaFor system;
          default = apertureFor system;
          cache-test = (pkgsFor system).runCommand "cache-test" { } ''
            mkdir -p $out/bin
            printf '#!/bin/sh\necho caustic-os-cache-ok\n' > $out/bin/cache-test
            chmod +x $out/bin/cache-test
          '';
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          devVm = self.nixosConfigurations.devVm.config.system.build.vm;
        }
        // nixpkgs.lib.optionalAttrs (system == "aarch64-linux") {
          productionImage = self.nixosConfigurations.production.config.system.build.image;
          devImage = self.nixosConfigurations.devImage.config.system.build.image;
        }
      );

      checks = perSystem (
        system:
        let
          pkgs = pkgsFor system;
          inherit (nixpkgs) lib;
        in
        {
          formatting = (treefmtEvalFor system).config.build.check self;
          pre-commit = preCommitHooksFor system;
          caustic-hardening = import ./checks/caustic-hardening.nix {
            inherit
              pkgs
              self
              nixpkgs
              lib
              ;
          };
          kernel-config = import ./checks/kernel-config.nix {
            inherit
              pkgs
              lib
              ;
          };
        }
      );

      tests = perSystem (system: {
        runtime = import ./checks/runtime-test.nix {
          pkgs = systemPkgsFor system;
          inherit self;
        };
        login = import ./checks/login-test.nix {
          pkgs = systemPkgsFor system;
          inherit self;
        };
        e2e = import ./checks/e2e-test.nix {
          pkgs = systemPkgsFor system;
          inherit self;
        };
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
              pkgs.sbctl
              pkgs.sbsigntool
              treefmt
            ];
            inherit (preCommitHooksFor system) shellHook;
          };
        }
      );

      formatter = perSystem (system: (treefmtEvalFor system).config.build.wrapper);
    };
}
