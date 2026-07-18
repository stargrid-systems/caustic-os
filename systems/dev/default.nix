# Dev NixOS configuration for fast iteration.
#
# Targets x86_64-linux so it boots as a QEMU VM on a developer laptop.
# The production CM4 image variant lives in a separate config and is
# aarch64-linux.
#
# Defaults are deliberately insecure (root SSH with password). This is
# for local VM use only. Production images disable SSH entirely.
{
  lib,
  pkgs,
  modulesPath,
  ...
}:
{
  imports = [
    # Provides `system.build.vm` and a virtual root filesystem so the
    # configuration evaluates without explicit disk/bootloader config.
    "${modulesPath}/virtualisation/qemu-vm.nix"
  ];

  system.stateVersion = "26.05";

  services = {
    # Aperture binds to [::]:80 (dual-stack, IPv6-preferred).
    aperture.enable = true;

    avahi = {
      enable = true;
      nssmdns4 = true;
      publish = {
        enable = true;
        addresses = true;
      };
    };

    # Minimal SSH server. Insecure settings below are dev-only.
    dropbear = {
      enable = true;
      allowPasswordAuth = true;
      allowRootLogin = true;
    };

    # Serial autologin for `system.run.vm` console convenience.
    getty.autologinUser = lib.mkDefault "root";
  };

  users.users.root.password = "caustic";

  networking = {
    # Explicit dual-stack. Linux address selection (RFC 6724) prefers
    # IPv6 by default when both are available.
    enableIPv6 = lib.mkDefault true;
    firewall = {
      enable = true;
      allowedTCPPorts = [ 80 ]; # aperture; dropbear opens its own port
    };
    useDHCP = lib.mkDefault true;
  };

  # Keep nix itself on the dev image so `nixos-rebuild test` works against
  # a local binary cache or flake.
  nix.package = pkgs.nix;
}
