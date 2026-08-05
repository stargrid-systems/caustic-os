{
  pkgs,
  lib,
}:
let
  baseKernel = pkgs.linux_6_12;

  configfile = pkgs.buildPackages.stdenv.mkDerivation {
    name = "kernel-config-verify-${baseKernel.version}";
    inherit (baseKernel) src;

    nativeBuildInputs = builtins.attrValues {
      inherit (pkgs.buildPackages)
        bc
        bison
        flex
        perl
        ;
    };

    buildPhase = ''
      make ARCH=arm64 defconfig
      cat ${../modules/kernel/config-enable} ${../modules/kernel/config-disable} >> .config
      make ARCH=arm64 olddefconfig
    '';

    installPhase = "cp .config $out";
  };

  requiredBuiltins = [
    "CONFIG_IPV6"
    "CONFIG_NET_VENDOR_BROADCOM"
    "CONFIG_BCMGENET"
    "CONFIG_BROADCOM_PHY"
    "CONFIG_PHYLIB"
    "CONFIG_MDIO_DEVICE"
    "CONFIG_MDIO_BCM_UNIMAC"

    "CONFIG_NF_TABLES"
    "CONFIG_NF_TABLES_INET"
    "CONFIG_NF_CONNTRACK"
    "CONFIG_NETFILTER_XTABLES"
    "CONFIG_IP_NF_IPTABLES"
    "CONFIG_IP_NF_FILTER"
    "CONFIG_IP6_NF_IPTABLES"
    "CONFIG_IP6_NF_FILTER"

    "CONFIG_BLK_DEV_DM"
    "CONFIG_DM_INIT"
    "CONFIG_DM_VERITY"
    "CONFIG_SQUASHFS"
    "CONFIG_EXT4_FS"

    "CONFIG_BLK_DEV_NVME"
    "CONFIG_NVME_CORE"
    "CONFIG_BLK_DEV_LOOP"

    "CONFIG_MMC"
    "CONFIG_MMC_BLOCK"
    "CONFIG_MMC_SDHCI"
    "CONFIG_MMC_SDHCI_IPROC"

    "CONFIG_PCIE_BRCMSTB"

    "CONFIG_SERIAL_AMBA_PL011"
    "CONFIG_SERIAL_AMBA_PL011_CONSOLE"

    "CONFIG_RASPBERRYPI_FIRMWARE"
    "CONFIG_BCM2835_WDT"

    "CONFIG_USB"
    "CONFIG_USB_STORAGE"
  ];
in
pkgs.runCommand "kernel-config-check"
  {
    inherit configfile;
    options = lib.concatStringsSep " " requiredBuiltins;
  }
  ''
    failures=0
    for opt in $options; do
      if ! grep -q "^''${opt}=y" "$configfile"; then
        echo "ERROR: $opt is not built-in (=y)"
        failures=$((failures + 1))
      fi
    done
    if [ "$failures" -gt 0 ]; then
      echo "$failures required kernel config option(s) missing"
      exit 1
    fi
    touch $out
  ''
