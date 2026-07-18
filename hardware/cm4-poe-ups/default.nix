# NixOS module for the Waveshare CM4-POE-UPS-BASE carrier board.
#
# Board specifics handled here:
#   * PCF85063a RTC on i2c-10 (i2c_csi_dsi) at 0x51
#   * EMC2301 fan controller on i2c-10 at 0x2f
#   * INA219 battery monitor on i2c-10 at 0x43 (read from userspace)
#   * Ethernet only: wireless modules blacklisted
#
# https://www.waveshare.com/wiki/CM4-POE-UPS-BASE
{ nixos-hardware }:
{
  lib,
  config,
  ...
}:
let
  cfg = config.hardware.caustic.cm4PoeUps;
in
{
  imports = [ "${nixos-hardware}/raspberry-pi/4" ];

  options.hardware.caustic.cm4PoeUps = {
    enable = lib.mkEnableOption "Waveshare CM4-POE-UPS-BASE carrier board support (RTC, fan controller, INA219 battery monitor)";
  };

  config = lib.mkIf cfg.enable {
    # Expose i2c-10 to userspace so the INA219 battery monitor can be read.
    hardware.i2c.enable = true;

    # Ethernet-only on this carrier. The CM4 variants we ship do not have
    # wireless silicon, but blacklist the modules anyway so a wireless CM4
    # variant never probes non-existent antennas.
    boot.blacklistedKernelModules = [
      "brcmfmac"
      "brcmutil"
      "bluetooth"
      "bnep"
      "btusb"
      "hci_uart"
    ];

    hardware.deviceTree = {
      enable = true;
      overlays = [
        {
          name = "cm4-poe-ups-rtc-pcf85063a";
          dtsText = ''
            /dts-v1/;
            /plugin/;
            / {
              compatible = "brcm,bcm2711";
              fragment@0 {
                target = <&i2c_csi_dsi>;
                __overlay__ {
                  #address-cells = <1>;
                  #size-cells = <0>;
                  status = "okay";
                  rtc@51 {
                    compatible = "nxp,pcf85063a";
                    reg = <0x51>;
                  };
                };
              };
            };
          '';
        }
        {
          name = "cm4-poe-ups-fan-emc2301";
          dtsText = ''
            /dts-v1/;
            /plugin/;
            / {
              compatible = "brcm,bcm2711";
              fragment@0 {
                target = <&i2c_csi_dsi>;
                __overlay__ {
                  #address-cells = <1>;
                  #size-cells = <0>;
                  status = "okay";
                  fanctrl@2f {
                    compatible = "microchip,emc2305";
                    reg = <0x2f>;
                    #cooling-cells = <2>;
                  };
                };
              };
            };
          '';
        }
      ];
    };
  };
}
