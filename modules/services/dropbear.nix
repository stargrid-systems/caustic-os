# Minimal SSH server suitable for memory-constrained appliances.
# Dropbear supports key and password auth with a much smaller footprint
# than openssh. Host keys persist under /var/lib/dropbear so they survive
# reboots.
{
  lib,
  pkgs,
  config,
  ...
}:
let
  cfg = config.services.dropbear;
  stateDir = "/var/lib/dropbear";

  # dropbear's three default host key names, written under StateDirectory
  # so they persist across boots.
  hostKeys = map (k: "${stateDir}/${k}") [
    "dropbear_rsa_host_key"
    "dropbear_ecdsa_host_key"
    "dropbear_ed25519_host_key"
  ];

  args = [
    "-F" # foreground, for systemd Type=simple
    "-E" # log to stderr, for journald
    "-R" # auto-generate host keys on first run
    "-p"
    (toString cfg.port)
  ]
  ++ lib.optional (!cfg.allowPasswordAuth) "-s"
  ++ lib.optional (!cfg.allowRootLogin) "-w"
  ++ builtins.concatMap (k: [
    "-r"
    k
  ]) hostKeys;
in
{
  options.services.dropbear = {
    enable = lib.mkEnableOption "dropbear SSH server";

    package = lib.mkPackageOption pkgs "dropbear" { };

    port = lib.mkOption {
      type = lib.types.port;
      default = 22;
      description = "TCP port dropbear listens on";
    };

    allowPasswordAuth = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to allow password authentication.
        Defaults to false (key-only auth) which is what production wants.
      '';
    };

    allowRootLogin = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to allow root login.
        Defaults to false. Enable for dev images only.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.dropbear = {
      description = "Dropbear SSH server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        Type = "simple";
        StateDirectory = "dropbear";
        ExecStart = "${lib.getExe cfg.package} ${lib.concatStringsSep " " args}";
        Restart = "on-failure";
        RestartSec = "5s";

        # Dropbear runs as root so it can read /etc/shadow, bind port 22,
        # and switch to the authenticated user. Keep the rest of the
        # sandbox tight.
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only"; # needed to read ~/.ssh/authorized_keys
        ReadWritePaths = [ stateDir ];
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        LockPersonality = true;
        SystemCallArchitectures = "native";
      };
    };

    networking.firewall.allowedTCPPorts = [ cfg.port ];
  };
}
