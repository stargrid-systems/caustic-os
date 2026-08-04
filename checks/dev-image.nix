{
  pkgs,
  self,
  lib,
  nixpkgs,
}:
let
  cfg =
    (nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        ({ modulesPath, ... }: {
          imports = [ "${modulesPath}/virtualisation/qemu-vm.nix" ];
        })
        self.nixosModules.caustic
        {
          caustic = {
            hardening.enable = true;
            networking.enable = true;
            users.enable = true;
          };

          users.users.root.hashedPassword = lib.mkForce "";
          networking.firewall.enable = lib.mkForce false;
          boot.kernel.sysctl = {
            "net.ipv4.conf.all.rp_filter" = lib.mkForce 0;
            "net.ipv4.conf.default.rp_filter" = lib.mkForce 0;
          };
        }
      ];
    }).config;
in
assert lib.assertMsg (
  cfg.users.users.root.hashedPassword == ""
) "devImage: root.hashedPassword must be empty for passwordless login";
assert lib.assertMsg (!cfg.networking.firewall.enable) "devImage: firewall must be disabled";
assert lib.assertMsg (
  cfg.boot.kernel.sysctl."net.ipv4.conf.all.rp_filter" == 0
) "devImage: rp_filter must be 0";
pkgs.runCommand "dev-image-check" { } ''
  touch $out
''
