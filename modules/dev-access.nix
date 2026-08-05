# Dev access: passwordless root login via SSH and serial console.
# INSECURE. Only for development images.
{
  lib,
  pkgs,
  ...
}:
{
  users.users.root.hashedPassword = "";

  services.dropbear = {
    enable = true;
    allowPasswordAuth = true;
    allowRootLogin = true;
    allowEmptyPasswords = true;
  };

  systemd.services."serial-getty@ttyAMA0".serviceConfig.ExecStart = [
    ""
    "-${lib.getExe' pkgs.util-linux "agetty"} --autologin root --noclear %I 115200 linux"
  ];

  networking.firewall.enable = false;

  boot.kernel.sysctl = {
    "net.ipv4.conf.all.rp_filter" = lib.mkForce 0;
    "net.ipv4.conf.default.rp_filter" = lib.mkForce 0;
  };
}
