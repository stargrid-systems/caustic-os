{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.boot.native-rpi;
  slotCfg = cfg.slot;
in
{
  options.boot.native-rpi.slot = {
    enable = lib.mkEnableOption "A/B slot management for native RPi boot";

    factoryResetSentinel = lib.mkOption {
      type = lib.types.path;
      default = "/persist/.factory-reset";
      description = "Sentinel file whose presence triggers a factory reset.";
    };
  };

  config = lib.mkIf (cfg.enable && slotCfg.enable) {
    systemd.services = {
      mark-slot-good = {
        description = "Mark current boot slot as good (commit A/B switch)";
        wantedBy = [ "multi-user.target" ];
        after = [ "multi-user.target" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        path = [ pkgs.util-linux ];
        script = ''
          set -euo pipefail

          isTryboot=$(cat /proc/device-tree/chosen/bootloader/tryboot 2>/dev/null | tr -d '\0' || echo "")
          if [ "$isTryboot" != "1" ]; then
            echo "mark-slot-good: normal boot, already committed"
            exit 0
          fi

          echo "mark-slot-good: tryboot detected, committing slot"
          mnt=$(mktemp -d)
          mount /dev/mmcblk0p1 "$mnt"
          autoboot="$mnt/autoboot.txt"

          if [ ! -f "$autoboot" ]; then
            echo "mark-slot-good: autoboot.txt not found on partition 1"
            umount "$mnt"
            rmdir "$mnt"
            exit 1
          fi

          curDefault=$(awk -F= '/^boot_partition=/ {print $2}' "$autoboot" | head -1)
          tryDefault=$(sed -n '/^\[tryboot\]/,/^\[/p' "$autoboot" | awk -F= '/^boot_partition=/ {print $2}' | head -1)

          tmp=$(mktemp)
          printf '[all]\ntryboot_a_b=1\nboot_partition=%s\n[tryboot]\nboot_partition=%s\n' \
            "$tryDefault" "$curDefault" > "$tmp"
          mv "$tmp" "$autoboot"
          sync
          umount "$mnt"
          rmdir "$mnt"
          echo "mark-slot-good: committed slot $tryDefault as default, old slot $curDefault is rollback"
        '';
      };

      factory-reset = {
        description = "Factory reset on sentinel";
        after = [ "persist.mount" ];
        before = [ "multi-user.target" ];
        requires = [ "persist.mount" ];
        unitConfig = {
          ConditionPathExists = slotCfg.factoryResetSentinel;
          DefaultDependencies = false;
        };
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          set -euo pipefail
          persistDir=$(dirname ${slotCfg.factoryResetSentinel})
          sentinelName=$(basename ${slotCfg.factoryResetSentinel})
          echo "factory-reset: wiping $persistDir"
          find "$persistDir" -mindepth 1 ! -name "$sentinelName" \
            -exec rm -rf {} + 2>/dev/null || true
          rm -f ${slotCfg.factoryResetSentinel}
          echo "factory-reset: complete, rebooting"
          systemctl reboot
        '';
      };
    };
  };
}
