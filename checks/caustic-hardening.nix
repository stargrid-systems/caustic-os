{
  pkgs,
  lib,
  self,
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
        }
      ];
    }).config;

  sysctl = cfg.boot.kernel.sysctl;
in
assert lib.assertMsg (sysctl."kernel.kptr_restrict" == 2) "kptr_restrict must be 2";
assert lib.assertMsg (sysctl."kernel.dmesg_restrict" == 1) "dmesg_restrict must be 1";
assert lib.assertMsg (
  sysctl."kernel.unprivileged_bpf_disabled" == 1
) "unprivileged_bpf_disabled must be 1";
assert lib.assertMsg (sysctl."kernel.sysrq" == 0) "sysrq must be 0";
assert lib.assertMsg (sysctl."kernel.yama.ptrace_scope" == 2) "ptrace_scope must be 2";
assert lib.assertMsg (sysctl."net.ipv4.conf.all.rp_filter" == 1) "rp_filter must be 1";
assert lib.assertMsg (sysctl."net.ipv4.tcp_syncookies" == 1) "tcp_syncookies must be 1";
assert lib.assertMsg cfg.security.apparmor.enable "AppArmor must be enabled";
assert lib.assertMsg (builtins.elem "bluetooth" cfg.boot.blacklistedKernelModules)
  "bluetooth must be blacklisted";
assert lib.assertMsg (builtins.elem "brcmfmac" cfg.boot.blacklistedKernelModules)
  "brcmfmac must be blacklisted";
assert lib.assertMsg (cfg.users.users.root.hashedPassword == "!") "root must be locked";
assert lib.assertMsg (builtins.elem 5353 cfg.networking.firewall.allowedUDPPorts)
  "mDNS must be allowed";
assert lib.assertMsg cfg.networking.firewall.allowPing "ICMP echo must be allowed";
pkgs.runCommand "caustic-hardening-check" { } ''
  touch $out
''
