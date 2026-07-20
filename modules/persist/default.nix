{ impermanence }:
{
  lib,
  config,
  ...
}:
let
  cfg = config.caustic.persist;
in
{
  imports = [ impermanence.nixosModules.impermanence ];

  options.caustic.persist = {
    enable = lib.mkEnableOption "persistence under /persist for read-only root";
  };

  config = lib.mkIf cfg.enable {
    environment.persistence."/persist" = {
      hideMounts = true;
      directories = [
        "/var/lib/aperture"
        "/var/lib/dropbear"
        "/var/lib/nixos"
        "/var/log/journal"
        "/nix/var/nix"
      ];
      files = [ "/etc/machine-id" ];
    };
  };
}
