{
  lib,
  config,
  ...
}:
let
  cfg = config.caustic.networking;
in
{
  options.caustic.networking = {
    enable = lib.mkEnableOption "production firewall defaults (mDNS, ICMP echo)";
  };

  config = lib.mkIf cfg.enable {
    networking.firewall = {
      enable = lib.mkDefault true;
      allowedUDPPorts = [ 5353 ];
      allowPing = true;
    };
  };
}
