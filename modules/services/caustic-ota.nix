{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.caustic-ota;
in
{
  options.services.caustic-ota = {
    enable = lib.mkEnableOption "caustic-ota update daemon";

    registry = lib.mkOption {
      type = lib.types.str;
      default = "ghcr.io/stargrid-systems/caustic-os";
      description = "OCI registry reference to poll for updates.";
    };

    tag = lib.mkOption {
      type = lib.types.str;
      default = "latest";
      description = "Tag to track in the registry.";
    };

    interval = lib.mkOption {
      type = lib.types.str;
      default = "1h";
      description = "Systemd OnUnitInactiveSec polling interval.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.caustic-ota = {
      description = "Caustic OS OTA update check";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${pkgs.caustic-ota}/bin/caustic-ota update --registry ${cfg.registry} --tag ${cfg.tag}";
        StateDirectory = "caustic-ota";
        ProtectSystem = "strict";
        ReadWritePaths = "/var/lib/caustic-ota /boot/EFI/Linux";
        CapabilityBoundingSet = "";
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
      };
    };

    systemd.timers.caustic-ota = {
      description = "Periodic caustic-ota update check";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "5min";
        OnUnitInactiveSec = cfg.interval;
        Persistent = true;
      };
    };
  };
}
