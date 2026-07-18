{
  lib,
  pkgs,
  config,
  ...
}:
let
  cfg = config.services.dropbear;
  stateDir = "/var/lib/dropbear";
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
      description = "Allow password authentication. Defaults to key-only.";
    };

    allowRootLogin = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Allow root login. Defaults to false.";
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

        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only"; # ~/.ssh/authorized_keys
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
