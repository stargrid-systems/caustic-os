{
  lib,
  pkgs,
  ...
}:
{
  system.stateVersion = "26.05";

  fileSystems = {
    "/" = {
      device = "/dev/disk/by-partlabel/root-a";
      fsType = "ext4";
      options = [ "ro" ];
      neededForBoot = true;
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
    users.enable = true;
    persist.enable = true;
  };

  services = {
    aperture.enable = true;

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
}
