{
  pkgs,
  lib,
  self,
}:
let
  inherit (pkgs.stdenv.hostPlatform) system;
in
pkgs.testers.runNixOSTest {
  name = "caustic-runtime";

  nodes.machine =
    {
      pkgs,
      config,
      ...
    }:
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
        recovery.enable = true;
        users.enable = true;
        persist.enable = true;
      };

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

      boot.initrd.systemd.enable = true;

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

      environment.systemPackages = [ pkgs.curl ];

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

      with subtest("persisted data survives reboot"):
          machine.succeed("echo survived > /var/lib/aperture/persist-test")
          machine.reboot()
          machine.wait_for_unit("multi-user.target")
          assert "survived" in machine.succeed("cat /var/lib/aperture/persist-test")
    '';
}
