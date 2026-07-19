# Dev VM config (x86_64-linux). Insecure by design: open access, no auth anywhere.
{
  lib,
  pkgs,
  modulesPath,
  ...
}:
{
  imports = [ "${modulesPath}/virtualisation/qemu-vm.nix" ];

  system.stateVersion = "26.05";

  boot.initrd.systemd.emergencyAccess = "";

  services = {
    aperture.enable = true;

    avahi = {
      enable = true;
      nssmdns4 = true;
      publish = {
        enable = true;
        addresses = true;
      };
    };

    dropbear = {
      enable = true;
      allowPasswordAuth = true;
      allowRootLogin = true;
      allowEmptyPasswords = true;
    };

    getty.autologinUser = lib.mkDefault "root";
  };

  users.users.root.password = lib.mkForce null;

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
