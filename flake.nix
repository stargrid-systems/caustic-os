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

    aperture-src = {
      url = "github:stargrid-systems/aperture/24071a1859211d04dff6e0f20c393a9f2a68f7ba";
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
      nixos-hardware,
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

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
          ]
          ++ lib.optional (system == "aarch64-linux") (
            _final: prev: {
              "auto-patchelf" = prev."auto-patchelf".overrideAttrs (old: {
                postInstall = (old.postInstall or "") + ''
                  sed -i '1 a import sys; sys.path.insert(0, "${prev.python3Packages.pyelftools}/lib/python${prev.python3.pythonVersion}/site-packages")' \
                    $out/bin/auto-patchelf
                '';
              });
            }
          );
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

      causticOtaFor =
        system:
        let
          craneLib = craneLibFor system;
          crate = craneLib.buildPackage {
            src = craneLib.path ./crates/caustic-ota;
            strictDeps = true;
            doCheck = false;
          };
        in
        crate.overrideAttrs (old: {
          meta = (old.meta or { }) // {
            mainProgram = "caustic-ota";
          };
        });

      osOverlay = final: prev: {
        aperture = apertureFor final.stdenv.hostPlatform.system;
        caustic-ota = causticOtaFor final.stdenv.hostPlatform.system;

        # nixpkgs wraps ukify with binutils in PATH but not sbsigntool.
        # ukify needs sbsign/sbverify when SecureBootPrivateKey is set, and
        # its wrapper overrides PATH, so nativeBuildInputs in callers don't help.
        systemdUkify = prev.systemdUkify.overrideAttrs (old: {
          postFixup = (old.postFixup or "") + ''
            wrapProgram $out/bin/ukify --prefix PATH : ${lib.makeBinPath [ final.sbsigntool ]}
          '';
        });
      };
      apertureOverlay = osOverlay;

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
          securebootKeys ? securebootKeysGlobal,
          imageId ? "caustic-os",
          otaRegistry ? "ghcr.io/stargrid-systems/caustic-os",
        }:
        nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = {
            inherit securebootKeys imageId otaRegistry;
          };
          modules = [
            ({ modulesPath, ... }: {
              imports = [
                "${modulesPath}/image/repart.nix"
                "${modulesPath}/image/repart-verity-store.nix"
                "${modulesPath}/system/boot/uki.nix"
                "${modulesPath}/system/boot/systemd/sysupdate.nix"
              ];
            })
            { nixpkgs.overlays = [ osOverlay ]; }
            self.nixosModules.cm4PoeUps
            self.nixosModules.aperture
            self.nixosModules.dropbear
            self.nixosModules.caustic
            self.nixosModules.causticOta
            self.nixosModules.persist
            ./systems/production/default.nix
            ./systems/production/updates.nix
          ];
        };

      securebootKeysGlobal =
        let
          keysEnvPath = builtins.getEnv "CAUSTIC_SECUREBOOT_KEYS";
          keysDir = if keysEnvPath != "" then keysEnvPath else "/usr/share/secureboot/keys/db";
          keyPath = "${keysDir}/db.key";
          certPath = "${keysDir}/db.crt";
        in
        # In pure eval, getEnv returns "" and the fallback path won't exist,
        # so this resolves to null. Use --impure with the env var (or rely on
        # the sbctl default location) to enable signing.
        # builtins.path copies the key files into the nix store so that
        # sandboxed derivations (sbsign, ukify) can read them.
        if keysEnvPath != "" && builtins.pathExists keyPath && builtins.pathExists certPath then
          {
            key = builtins.path {
              path = keyPath;
              name = "secureboot-db.key";
            };
            cert = builtins.path {
              path = certPath;
              name = "secureboot-db.crt";
            };
          }
        else
          null;
    in
    {
      overlays.default = apertureOverlay;

      nixosModules = {
        cm4PoeUps = import ./hardware/cm4-poe-ups { inherit nixos-hardware; };
        aperture = import ./modules/services/aperture.nix;
        dropbear = import ./modules/services/dropbear.nix;
        caustic = import ./modules/caustic;
        causticOta = import ./modules/services/caustic-ota.nix;
        persist = import ./modules/persist { inherit impermanence; };
      };

      nixosConfigurations = {
        dev = devNixosFor "x86_64-linux";
        production = prodNixosFor "aarch64-linux" { };
        devImage = prodNixosFor "aarch64-linux" {
          securebootKeys = null;
          imageId = "caustic-os-dev";
          otaRegistry = "ghcr.io/stargrid-systems/caustic-os-dev";
        };
      };

      packages = perSystem (
        system:
        {
          aperture = apertureFor system;
          caustic-ota = causticOtaFor system;
          default = apertureFor system;
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          devVm = self.nixosConfigurations.dev.config.system.build.vm;
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
        }
      );

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
