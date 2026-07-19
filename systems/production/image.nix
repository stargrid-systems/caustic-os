{
  lib,
  config,
  pkgs,
  ...
}:
let
  rpiFw = "${pkgs.raspberrypifw}/share/raspberrypi/boot";

  rpi4Uefi = pkgs.fetchzip {
    url = "https://github.com/pftf/RPi4/releases/download/v1.52/RPi4_UEFI_Firmware_v1.52.zip";
    hash = "sha256-nL/fKtVzxpaIjiy0nCG/K94/nN5jG2Bzae3d3tUoIMo=";
    stripRoot = false;
  };

  configTxt = pkgs.writeText "config.txt" ''
    arm_64bit=1
    arm_boost=1
    enable_uart=1
    uart_2ndstage=1
    enable_gic=1
    armstub=RPI_EFI.fd
    disable_commandline_tags=1
    disable_overscan=1
    device_tree_address=0x3e0000
    device_tree_end=0x400000
  '';

  systemdBoot = "${pkgs.systemd}/lib/systemd/boot/efi/systemd-boot${pkgs.stdenv.hostPlatform.efiArch}.efi";
  bootEfiName = "BOOT${lib.toUpper pkgs.stdenv.hostPlatform.efiArch}.EFI";
  systemdEfiName = "systemd-boot${pkgs.stdenv.hostPlatform.efiArch}.efi";

  loaderConf = pkgs.writeText "loader.conf" ''
    timeout 3
    default @saved
    editor no
  '';

  version = config.system.image.version;
in
{
  image.repart = {
    name = "caustic-os";

    split = true;

    verityStore = {
      enable = true;
      partitionIds = {
        esp = "00-esp";
        store-verity = "10-store-verity";
        store = "20-store";
      };
    };

    compression = {
      enable = true;
      algorithm = "zstd";
      level = 9;
    };

    partitions = {
      "00-esp" = {
        repartConfig = {
          Type = "esp";
          Format = "vfat";
          Label = "ESP";
          SizeMinBytes = "256M";
          SplitName = "-";
        };
        contents = {
          "/RPI_EFI.fd".source = "${rpi4Uefi}/RPI_EFI.fd";
          "/config.txt".source = configTxt;
          "/start4.elf".source = "${rpiFw}/start4.elf";
          "/fixup4.dat".source = "${rpiFw}/fixup4.dat";
          "/bcm2711-rpi-cm4.dtb".source =
            "${config.hardware.deviceTree.package}/broadcom/bcm2711-rpi-cm4.dtb";
          "/EFI/BOOT/${bootEfiName}".source = systemdBoot;
          "/EFI/systemd/${systemdEfiName}".source = systemdBoot;
          "/loader/loader.conf".source = loaderConf;
        };
      };

      "10-store-verity".repartConfig = {
        SizeMinBytes = "64M";
        SizeMaxBytes = "64M";
        Label = "verity-${version}";
        SplitName = "verity";
        ReadOnly = 1;
      };
      "20-store".repartConfig = {
        Minimize = "best";
        Label = "usr-${version}";
        SplitName = "usr";
        ReadOnly = 1;
      };

      "30-persist" = {
        repartConfig = {
          Type = "linux-generic";
          Format = "ext4";
          Label = "persist";
          SizeMinBytes = "1G";
          SplitName = "-";
        };
      };
    };
  };
}
