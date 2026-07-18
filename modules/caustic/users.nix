{
  lib,
  config,
  ...
}:
let
  cfg = config.caustic.users;
in
{
  options.caustic.users = {
    enable = lib.mkEnableOption "production user policy (locked root)";
  };

  config = lib.mkIf cfg.enable {
    users.users.root.hashedPassword = "!";
  };
}
