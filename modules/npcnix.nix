{ self }:
{ config, pkgs, lib, ... }:
with lib;
{
  options.npcnix = {
    enable = mkOption {
      default = true;
      type = types.bool;
      description = lib.mdDoc ''
        Enable or disable the npcnix controlling this system.
      '';
    };

    package = mkOption {
      type = types.package;
      default = self.outputs.packages.${pkgs.system}.npcnix;
      description = mdDoc "The package providing npcnix binary.";
    };
  };

  config = mkIf config.npcnix.enable {
    environment.systemPackages = [ config.npcnix.package ];

    systemd.services.npcnix = {
      # restart after successful activation to reload itself, without blocking/terminating whole system activation
      script = ''
        ${config.npcnix.package}/bin/npcnix follow --once=activate
      '';

      wantedBy = [ "multi-user.target" ];
      after = [ "multi-user.target" ];

      stopIfChanged = false;
      reloadIfChanged = false; # no reload type
      restartIfChanged = false; # we don't want to kill daemon currently running `nixos-rebuild`

      serviceConfig = {
        Restart = "always";
        RestartSec = 15;
      };
    };
  };
}
