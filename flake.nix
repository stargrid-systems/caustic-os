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
  };

  outputs =
    {
      self,
      nixpkgs,
      pre-commit-hooks-nix,
      treefmt-nix,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      perSystem = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = system: nixpkgs.legacyPackages.${system};

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
    in
    {
      nixosModules = { };
      nixosConfigurations = { };
      packages = perSystem (_: { });

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
