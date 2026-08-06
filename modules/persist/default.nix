{ impermanence }:
{
  lib,
  config,
  ...
}:
let
  cfg = config.caustic.persist;

  directories = [
    "/var/lib/aperture"
    "/var/lib/caustic-ota"
    "/var/lib/dropbear"
    "/var/lib/nixos"
    "/var/lib/systemd"
    "/var/log/journal"
    "/nix/var/nix"
  ];
in
{
  imports = [ impermanence.nixosModules.impermanence ];

  options.caustic.persist = {
    enable = lib.mkEnableOption "persistence under /persist for read-only root";
  };

  config = lib.mkIf cfg.enable {
    environment.persistence."/persist" = {
      hideMounts = true;
      inherit directories;
      files = [ "/etc/machine-id" ];
    };
  };
}
