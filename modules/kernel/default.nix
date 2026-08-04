{
  lib,
  pkgs,
  ...
}:

let
  baseKernel = pkgs.linux_6_12;

  kernelConfig = pkgs.buildPackages.stdenv.mkDerivation {
    name = "linux-config-${baseKernel.version}";
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
      cat ${./config-enable} ${./config-disable} >> .config
      make ARCH=arm64 olddefconfig
    '';

    installPhase = "cp .config $out";
  };

  buildLinux = pkgs.callPackage "${pkgs.path}/pkgs/os-specific/linux/kernel/build.nix" { };

  customKernel = buildLinux {
    pname = "linux-caustic";
    inherit (baseKernel) version src modDirVersion;
    kernelPatches = baseKernel.kernelPatches or [ ];
    configfile = kernelConfig;
    config = {
      CONFIG_MODULES = "y";
      CONFIG_RUST = "n";
    };
  };
in
{
  boot = {
    kernelPackages = lib.mkDefault (pkgs.linuxKernel.packagesFor customKernel);
  };
}
