{
  lib,
  config,
  pkgs,
  imageId ? "caustic-os",
  otaRegistry ? "ghcr.io/stargrid-systems/caustic-os",
  ...
}:
let
  versionFile = lib.fileContents ../../version.txt;
in
{
  config = {
    system = {
      stateVersion = "26.05";
      image = {
        id = lib.mkDefault imageId;
        version = lib.mkDefault (lib.strings.trim versionFile);
      };
      nixos = {
        distroName = "Caustic OS";
        distroId = "caustic-os";
        extraOSReleaseArgs = {
          PRETTY_NAME = "Caustic OS ${config.system.image.version}";
          VERSION = config.system.image.version;
          VERSION_ID = config.system.image.version;
        };
      };
    };

    services.caustic-ota.registry = lib.mkDefault otaRegistry;

    boot = {
      kernelPackages = pkgs.linuxPackages_6_12;
      native-rpi = {
        enable = true;
        slot.enable = true;
      };
    };

    hardware.deviceTree.enable = true;

    fileSystems = {
      "/boot/a" = {
        device = "/dev/mmcblk0p1";
        fsType = "vfat";
        options = [
          "rw"
          "nofail"
        ];
      };
      "/boot/b" = {
        device = "/dev/mmcblk0p2";
        fsType = "vfat";
        options = [
          "rw"
          "nofail"
        ];
      };
    };

    caustic = {
      hardening.enable = true;
      networking.enable = true;
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
