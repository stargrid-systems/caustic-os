{
  lib,
  pkgs,
  config,
  ...
}:
let
  cfg = config.services.aperture;
in
{
  options.services.aperture = {
    enable = lib.mkEnableOption "aperture gateway service";

    package = lib.mkPackageOption pkgs "aperture" { };

    httpAddr = lib.mkOption {
      type = lib.types.str;
      default = "[::]:80";
      example = "[::]:8080";
      description = ''
        HTTP listener address. HTTP serves the full API when no HTTPS
        listener is configured, or redirects to HTTPS when one is.
      '';
    };

    httpsAddr = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "[::]:443";
      description = "HTTPS listener address. null disables HTTPS.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/aperture";
      defaultText = lib.literalExpression "/var/lib/aperture";
      description = "Aperture's single managed data directory (DB, certs, runtime state).";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.aperture = {
      isSystemUser = true;
      group = "aperture";
      home = cfg.dataDir;
      createHome = true;
    };
    users.groups.aperture = { };

    systemd.services.aperture = {
      description = "Aperture gateway";
      documentation = [ "https://github.com/stargrid-systems/aperture" ];
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        User = "aperture";
        Group = "aperture";
        StateDirectory = "aperture";
        RuntimeDirectory = "aperture";
        ExecStart = builtins.concatStringsSep " " (
          [
            (lib.getExe' cfg.package "aperture")
            "run"
            "--http-addr"
            cfg.httpAddr
            "--data-dir"
            cfg.dataDir
          ]
          ++ lib.optionals (cfg.httpsAddr != null) [
            "--https-addr"
            cfg.httpsAddr
          ]
        );
        Restart = "on-failure";
        RestartSec = "5s";

        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = false;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" ];
        # Ports below 1024.
        AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
        CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];
      };
    };
  };
}
