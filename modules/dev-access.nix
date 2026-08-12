# Dev access: passwordless root via SSH and an automatic root shell on the
# serial console.
# INSECURE. Only for development images.
{
  lib,
  pkgs,
  ...
}:
let
  # Root shell without login(8)/PAM. The appliance image has no setuid
  # unix_chkpwd helper, so password auth via pam_unix does not work.
  serialShell = pkgs.writeShellScript "dev-serial-shell" ''
    export HOME=/root
    export USER=root
    export LOGNAME=root
    exec ${lib.getExe pkgs.bashInteractive} --login
  '';
in
{
  users.users.root.password = "";

  services.dropbear = {
    enable = true;
    allowPasswordAuth = true;
    allowRootLogin = true;
    allowEmptyPasswords = true;
  };

  systemd.services."serial-getty@".serviceConfig.ExecStart = [
    ""
    "-${lib.getExe' pkgs.util-linux "agetty"} --autologin root --noclear --login-program ${serialShell} %I 115200 linux"
  ];

  networking.firewall.enable = false;

  boot.kernel.sysctl = {
    "net.ipv4.conf.all.rp_filter" = lib.mkForce 0;
    "net.ipv4.conf.default.rp_filter" = lib.mkForce 0;
  };
}
