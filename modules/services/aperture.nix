# NixOS module for the aperture gateway service.
#
# Bundles aperture as a systemd service with strict hardening.
# Storage layout: aperture owns /var/lib/aperture exclusively and manages
# its own DBs, TLS material, and runtime state under that path.
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

    addr = lib.mkOption {
      type = lib.types.str;
      default = "[::]:80";
      example = "[::]:443";
      description = ''
        Socket address aperture will bind to, in `[ip]:port` form.
        Use `[::]:` to bind on all interfaces (dual-stack: IPv6 with
        IPv4-mapped addresses).
        Binding to ports below 1024 is supported via
        `CAP_NET_BIND_SERVICE` granted to the service.
      '';
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/aperture";
      defaultText = lib.literalExpression "/var/lib/aperture";
      description = ''
        Path to aperture's single managed data directory.
        Contains the libSQL database, TLS material, and any other
        runtime state aperture needs to persist.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.aperture = {
      isSystemUser = true;
      group = "aperture";
      home = cfg.dataDir;
      createHome = true;
      description = "Aperture gateway user";
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
        ExecStart = "${lib.getExe' cfg.package "aperture"} run --addr ${cfg.addr} --data-dir ${cfg.dataDir}";
        Restart = "on-failure";
        RestartSec = "5s";

        # Hardening: no privileges, no escape vectors.
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
        # Required to bind ports below 1024 (e.g. 80, 443).
        AmbientCapabilities = [ "CAP_NET_BIND_SERVICE" ];
        CapabilityBoundingSet = [ "CAP_NET_BIND_SERVICE" ];
      };
    };
  };
}
