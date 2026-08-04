{
  pkgs,
  self,
}:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) system;
  isAarch64 = system == "aarch64-linux";

  realImageBuilds = lib.optionals isAarch64 [
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
      ]
      ++ lib.optionals isAarch64 [
        self.nixosModules.kernel
      ];

      caustic = {
        hardening.enable = true;
        networking.enable = true;
        users.enable = true;
        persist.enable = true;
      };

      boot.kernelPackages = lib.mkIf (!isAarch64) pkgs.linuxPackages;

      # qemu-vm.nix populates these with x86-only modules (ata_piix, atkbd)
      # and modules that are built-in in our kernel (dm_mod, loop, xhci_pci).
      # Override with only modules that exist as .ko files in our kernel.
      boot.initrd.availableKernelModules = lib.mkIf isAarch64 (lib.mkForce [
        "virtio_blk"
        "virtio_net"
        "virtio_mmio"
        "9p"
        "9pnet_virtio"
        "virtio_rng"
      ]);
      boot.initrd.kernelModules = lib.mkIf isAarch64 (lib.mkForce [
        "virtio_blk"
        "virtio_net"
        "virtio_console"
        "virtio_rng"
      ]);
      boot.kernelModules = lib.mkIf isAarch64 (lib.mkForce [ ]);

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
