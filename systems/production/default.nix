{
  lib,
  pkgs,
  securebootKeys ? null,
  ...
}:
let
  versionFile = lib.fileContents ../../version.txt;
in
{
  imports = [
    ./image.nix
    ./update-package.nix
    ./updates.nix
  ];

  options.caustic.secureboot.keys = lib.mkOption {
    type = lib.types.nullOr (
      lib.types.submodule {
        options = {
          key = lib.mkOption { type = lib.types.path; };
          cert = lib.mkOption { type = lib.types.path; };
        };
      }
    );
    default = securebootKeys;
    internal = true;
    description = "Secure Boot signing keys for UKI and systemd-boot. null disables signing.";
  };

  config = {
    system = {
      stateVersion = "26.05";
      image = {
        id = "caustic-os";
        version = lib.mkDefault (lib.strings.trim versionFile);
      };
    };

    boot = {
      loader = {
        grub.enable = false;
        generic-extlinux-compatible.enable = false;
      };
      initrd.systemd.enable = true;
      uki.settings.UKI = lib.optionalAttrs (securebootKeys != null) {
        SecureBootCertificate = securebootKeys.cert;
        SecureBootPrivateKey = securebootKeys.key;
      };
    };

    hardware.deviceTree.enable = true;

    fileSystems = {
      "/" = {
        fsType = "tmpfs";
        options = [
          "mode=755"
          "size=50%"
        ];
        neededForBoot = true;
      };
      "/boot" = {
        device = "/dev/disk/by-partlabel/ESP";
        fsType = "vfat";
        options = [
          "rw"
          "nofail"
        ];
      };
      "/persist" = {
        device = "/dev/disk/by-partlabel/persist";
        fsType = "ext4";
        neededForBoot = true;
      };
    };

    caustic = {
      hardening.enable = true;
      networking.enable = true;
      recovery.enable = true;
      users.enable = true;
      persist.enable = true;
    };

    services = {
      aperture.enable = true;
      caustic-ota.enable = true;

      avahi = {
        enable = true;
        publish = {
          enable = true;
          addresses = true;
        };
      };
    };

    networking = {
      enableIPv6 = lib.mkDefault true;
      firewall = {
        enable = true;
        allowedTCPPorts = [ 80 ];
      };
      useDHCP = lib.mkDefault true;
    };

    nix.package = pkgs.nix;
  };
}
