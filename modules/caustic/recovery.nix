{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.caustic.recovery;
in
{
  options.caustic.recovery = {
    enable = lib.mkEnableOption "recovery and factory reset support";

    factoryResetSentinel = lib.mkOption {
      type = lib.types.path;
      default = "/persist/.factory-reset";
      description = ''
        Path to a sentinel file whose presence triggers a factory reset
        on the next boot. The initrd wipes persist contents when the
        sentinel exists, then removes it.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    boot = {
      kernelParams = [
        "console=ttyAMA0,115200"
        "console=tty1"
        "systemd.show_status=1"
        "systemd.log_level=warn"
      ];

      initrd.systemd = {
        services.factory-reset = {
          description = "Factory reset on sentinel";
          wantedBy = [ "initrd.target" ];
          after = [ "sysroot-persist.mount" ];
          before = [
            "sysroot.mount"
            "initrd-fs.target"
          ];
          unitConfig = {
            ConditionPathExists = cfg.factoryResetSentinel;
            DefaultDependencies = false;
          };
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
          };
          script = ''
            persist_dir=$(dirname ${cfg.factoryResetSentinel})
            sentinel_name=$(basename ${cfg.factoryResetSentinel})
            echo "caustic: factory reset requested, wiping $persist_dir"
            find "$persist_dir" -mindepth 1 ! -name "$sentinel_name" \
              -exec rm -rf {} + 2>/dev/null || true
            rm -f ${cfg.factoryResetSentinel}
            echo "caustic: factory reset complete"
          '';
        };
      };
    };

    systemd.services.mark-boot-good = {
      description = "Mark current boot as successful for sysupdate rollback";
      wantedBy = [ "multi-user.target" ];
      after = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${pkgs.systemd}/bin/bootctl set-good";
      };
    };
  };
}
