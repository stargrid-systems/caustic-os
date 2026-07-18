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
    enable = lib.mkEnableOption "Waveshare CM4-POE-UPS-BASE (RTC, fan, INA219)";
  };

  config = lib.mkIf cfg.enable {
    hardware.i2c.enable = true;

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
