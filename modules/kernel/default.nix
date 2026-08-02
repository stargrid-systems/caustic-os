{
  lib,
  pkgs,
  ...
}:

let
  baseKernel = pkgs.linux_6_12;

  # Generate .config from arm64 defconfig + our overrides.
  # Uses the kernel's own olddefconfig to resolve dependencies.
  # This avoids nixpkgs' generate-config.pl which conflicts with MODULES=n.
  # Runs on the build platform since config generation only uses host tools.
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
      cat ${./config-overrides} >> .config
      make ARCH=arm64 olddefconfig
    '';

    installPhase = "cp .config $out";
  };

  # Call build.nix directly instead of going through generic.nix.
  # generic.nix hardcodes config.CONFIG_MODULES = "y" which makes
  # build.nix think the kernel is modular and run postInstall
  # (modules_install, Module.symvers copy, etc.) that fails with MODULES=n.
  buildLinux = pkgs.callPackage "${pkgs.path}/pkgs/os-specific/linux/kernel/build.nix" { };

  customKernel = buildLinux {
    pname = "linux-caustic";
    inherit (baseKernel) version src modDirVersion;
    kernelPatches = baseKernel.kernelPatches or [ ];
    configfile = kernelConfig;
    config = {
      CONFIG_MODULES = "n";
      CONFIG_RUST = "n";
    };
  };
in
{
  boot = {
    kernelPackages = lib.mkDefault (pkgs.linuxKernel.packagesFor customKernel);
    kernelModules = lib.mkForce [ ];
  };
}
