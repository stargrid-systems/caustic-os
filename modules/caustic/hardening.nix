{
  lib,
  config,
  ...
}:
let
  cfg = config.caustic.hardening;
in
{
  options.caustic.hardening = {
    enable = lib.mkEnableOption "kernel, sysctl, and MAC hardening";
  };

  config = lib.mkIf cfg.enable {
    boot = {
      kernelParams = [
        "page_poison=1"
        "slab_nomerge"
        "slub_debug=P"
        "init_on_alloc=1"
        "init_on_free=1"
      ];

      blacklistedKernelModules = [
        "bluetooth"
        "bnep"
        "btusb"
        "hci_uart"
        "brcmfmac"
        "brcmutil"
        "snd_bcm2835"
        "snd_seq"
        "snd_seq_midi"
        "snd_rawmidi"
      ];

      kernel.sysctl = {
        "kernel.kptr_restrict" = 2;
        "kernel.dmesg_restrict" = 1;
        "kernel.perf_event_paranoid" = 2;
        "kernel.unprivileged_bpf_disabled" = 1;
        "kernel.sysrq" = 0;
        "kernel.yama.ptrace_scope" = 2;

        "net.ipv4.conf.all.rp_filter" = 1;
        "net.ipv4.conf.default.rp_filter" = 1;
        "net.ipv4.conf.all.accept_redirects" = 0;
        "net.ipv4.conf.default.accept_redirects" = 0;
        "net.ipv4.conf.all.send_redirects" = 0;
        "net.ipv4.conf.default.send_redirects" = 0;
        "net.ipv4.conf.all.accept_source_route" = 0;
        "net.ipv4.conf.default.accept_source_route" = 0;
        "net.ipv4.tcp_syncookies" = 1;

        "net.ipv6.conf.all.accept_ra" = 0;
        "net.ipv6.conf.default.accept_ra" = 0;
        "net.ipv6.conf.all.accept_redirects" = 0;
        "net.ipv6.conf.default.accept_redirects" = 0;
      };
    };

    security.apparmor.enable = true;
  };
}
