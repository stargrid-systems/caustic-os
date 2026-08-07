{
  pkgs,
  self,
}:
pkgs.testers.runNixOSTest {
  name = "caustic-login-test";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [
        self.nixosModules.caustic
        self.nixosModules.dropbear
        self.nixosModules.dev-access
      ];

      caustic = {
        hardening.enable = true;
        networking.enable = true;
        users.enable = true;
      };

      boot.kernelPackages = pkgs.linuxPackages;

      virtualisation = {
        cores = 1;
        memorySize = 512;
      };

      environment.systemPackages = [ pkgs.sshpass ];

      system.stateVersion = "26.05";
    };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("dropbear is running and listening on port 22"):
        machine.wait_for_unit("dropbear.service")
        machine.wait_for_open_port(22)

    with subtest("root can log in via SSH with empty password"):
        machine.succeed(
            "sshpass -p ''' ssh -o StrictHostKeyChecking=no "
            "-o UserKnownHostsFile=/dev/null root@localhost 'echo SSH_OK'"
        )

    with subtest("serial-getty has auto-login for root"):
        _, content = machine.execute("cat /etc/systemd/system/serial-getty@ttyAMA0.service 2>&1")
        assert "autologin root" in content, (
            f"serial-getty@ttyAMA0.service missing autologin root. Content:\n{content}"
        )
  '';
}
