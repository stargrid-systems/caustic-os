{
  pkgs,
  self,
}:
let
  inherit (pkgs) lib runCommand;
  inherit (pkgs.stdenv.hostPlatform) system;

  # Force the real dev/prod systems to build on aarch64 so
  # kernel/initrd/module regressions fail CI instead of surfacing
  # only at flash time.
  realImageBuilds = lib.optional (system == "aarch64-linux") (
    runCommand "caustic-image-builds" { } ''
      echo ${self.nixosConfigurations.devImage.config.system.build.toplevel}
      echo ${self.nixosConfigurations.production.config.system.build.toplevel}
      touch $out
    ''
  );
in
pkgs.testers.runNixOSTest {
  name = "caustic-runtime";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [
        self.nixosModules.caustic
        self.nixosModules.aperture
        self.nixosModules.dropbear
        self.nixosModules.causticOta
        self.nixosModules.persist
      ];

      caustic = {
        hardening.enable = true;
        networking.enable = true;
        users.enable = true;
        persist.enable = true;
      };

      # Match the production/devImage kernel so the test exercises
      # the real kernel the device boots, not the nixpkgs default.
      boot.kernelPackages = pkgs.linuxPackages_7_1;

      users.users.root.hashedPassword = pkgs.lib.mkForce null;

      services = {
        aperture = {
          enable = true;
          package = self.packages.${system}.aperture;
        };
        caustic-ota = {
          enable = true;
          package = self.packages.${system}.caustic-ota;
        };
        dropbear.enable = true;
      };

      virtualisation = {
        cores = 2;
        memorySize = 1024;
        emptyDiskImages = [ 512 ];
      };

      networking.useDHCP = true;

      fileSystems."/persist" = {
        device = "/dev/vdb";
        autoFormat = true;
        fsType = "ext4";
        neededForBoot = true;
      };

      environment.systemPackages = [ pkgs.curl ] ++ realImageBuilds;

      system.stateVersion = "26.05";
    };

  testScript = # python
    ''
      machine.start()
      machine.wait_for_unit("multi-user.target")

      with subtest("aperture serves on port 80"):
          machine.wait_for_unit("aperture.service")
          machine.wait_for_open_port(80)
          machine.succeed("curl -s -o /dev/null http://localhost:80/")

      with subtest("dropbear listens on port 22"):
          machine.wait_for_unit("dropbear.service")
          machine.wait_for_open_port(22)

      with subtest("caustic-ota timer is active"):
          machine.wait_for_unit("caustic-ota.timer")

      with subtest("impermanence bind-mounts /persist into service dirs"):
          machine.succeed("echo survived > /var/lib/aperture/persist-test")
          machine.succeed("test -f /persist/var/lib/aperture/persist-test")
          assert "survived" in machine.succeed("cat /persist/var/lib/aperture/persist-test")
    '';
}
