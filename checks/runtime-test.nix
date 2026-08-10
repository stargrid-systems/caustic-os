{
  pkgs,
  self,
}:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) system;

  realImageBuilds = lib.optionals (system == "aarch64-linux") [
    self.nixosConfigurations.devImage.config.system.build.toplevel
    self.nixosConfigurations.production.config.system.build.toplevel
  ];
in
pkgs.testers.runNixOSTest {
  name = "caustic-test";

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

      boot.kernelPackages = pkgs.linuxPackages;

      users.users.root.hashedPassword = null;

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
        avahi = {
          enable = true;
          publish = {
            enable = true;
            addresses = true;
          };
        };
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

      system.extraDependencies = realImageBuilds;

      environment.systemPackages = [
        pkgs.curl
        pkgs.nftables
      ];

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

      with subtest("aperture publishes mDNS service files"):
          machine.wait_for_unit("avahi-daemon.service")
          http = machine.succeed("cat /etc/avahi/services/aperture-http.service")
          assert "_http._tcp" in http and "<port>80</port>" in http
          https = machine.succeed("cat /etc/avahi/services/aperture-https.service")
          assert "_https._tcp" in https and "<port>443</port>" in https

      with subtest("dropbear listens on port 22"):
          machine.wait_for_unit("dropbear.service")
          machine.wait_for_open_port(22)

      with subtest("caustic-ota timer is active"):
          machine.wait_for_unit("caustic-ota.timer")

      with subtest("impermanence bind-mounts /persist into service dirs"):
          machine.succeed("echo survived > /var/lib/aperture/persist-test")
          machine.succeed("test -f /persist/var/lib/aperture/persist-test")
          assert "survived" in machine.succeed("cat /persist/var/lib/aperture/persist-test")

      with subtest("firewall is active with nftables rules"):
          machine.wait_for_unit("firewall.service")
          machine.succeed("nft list ruleset")

      with subtest("IPv6 is enabled"):
          machine.succeed("ip -6 addr show dev lo | grep -q inet6")

      with subtest("sysctl hardening is applied"):
          assert "2" in machine.succeed("cat /proc/sys/kernel/kptr_restrict")
          assert "1" in machine.succeed("cat /proc/sys/kernel/dmesg_restrict")
          assert "0" in machine.succeed("cat /proc/sys/kernel/sysrq")
    '';
}
