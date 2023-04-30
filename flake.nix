{
  description = "dpc's basic flake template";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?rev=f294325aed382b66c7a188482101b0f336d1d7db"; # nixos-unstable
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane?rev=445a3d222947632b5593112bb817850e8a9cf737"; # v0.12.1
    crane.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, crane }: {
    nixosModules = {
      npcnix = import ./modules/npcnix.nix { inherit self; };
      default = self.nixosModules.npcnix;
    };

    nixosConfigurations =
      let
        system = "x86_64-linux";
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      {
        basic = nixpkgs.lib.nixosSystem {
          inherit system pkgs;

          modules = [

            ({ modulesPath, ... }: {
              imports = [ "${modulesPath}/virtualisation/amazon-image.nix" ];
              ec2.hvm = true;


              system.stateVersion = "22.11";
            })

            self.nixosModules.default
          ];
        };
      };
  }
  // flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs {
        inherit system;
      };
      lib = pkgs.lib;
      craneLib = crane.lib.${system};

      commonArgs =

        let
          # Only keeps markdown files
          readmeFilter = path: _type: builtins.match ".*/README\.md$" path != null;
          markdownOrCargo = path: type:
            (readmeFilter path type) || (craneLib.filterCargoSources path type);
        in
        {
          doCheck = false;

          src = lib.cleanSourceWith {
            src = craneLib.path ./.;
            filter = markdownOrCargo;
          };

          buildInputs = [
            pkgs.openssl
            pkgs.pkg-config
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.darwin.apple_sdk.frameworks.Security
          ];

          nativeBuildInputs = [ ];
        };
      npcnixPkgUnwrapped = craneLib.buildPackage ({ } // commonArgs);
      npcnixPkgWrapped = pkgs.writeShellScriptBin "npcnix" ''
        exec env \
          NPCNIX_AWS_CLI=''${NPCNIX_AWS_CLI:-${pkgs.awscli2}/bin/aws} \
          NPCNIX_NIXOS_REBUILD=''${NPCNIX_NIXOS_REBUILD:-${pkgs.nixos-rebuild}/bin/nixos-rebuild} \
          PATH="${pkgs.git}/bin:$PATH" \
          ${npcnixPkgUnwrapped}/bin/npcnix "$@"
      '';
    in
    {
      packages = {
        default = npcnixPkgWrapped;
        npcnix-unwrapped = npcnixPkgUnwrapped;
        npcnix = npcnixPkgWrapped;
        install = pkgs.writeShellScriptBin "npcnix-install" ''
          set -e
          npcnix_swapfile="/npcnix-install-swapfile"

          function cleanup() {
            if [ -e "$npcnix_swapfile" ]; then
              >&2 echo "Cleaning up temporary swap file..."
              ${pkgs.util-linux}/bin/swapoff "$npcnix_swapfile" || true
              ${pkgs.coreutils}/bin/rm -f "$npcnix_swapfile" || true
            fi
          }
          # clean unconditionally, in case we left over something in a previous run, etc.
          trap cleanup EXIT

          if [ "$(${pkgs.util-linux}/bin/swapon --noheadings --raw | ${pkgs.coreutils}/bin/wc -l )" = "0" ] ; then
            >&2 echo "No swap detected. Creating a temporary swap file..."
            if [ ! -e "$npcnix_swapfile" ]; then
              # it has been experimentally verified, that 2G should be enough
              # bootstrap even on AWS EC2 t3.nano instances
              ${pkgs.util-linux}/bin/fallocate -l 2G "$npcnix_swapfile" || true
            fi
            chmod 600 "$npcnix_swapfile" && \
              ${pkgs.util-linux}/bin/mkswap "$npcnix_swapfile" && \
              ${pkgs.util-linux}/bin/swapon "$npcnix_swapfile" || \
              true
          fi

          ${npcnixPkgWrapped}/bin/npcnix install "$@"
          cleanup
        '';
      };

      checks = {
        nixosConfiguration = self.nixosConfigurations.basic.config.system.build.toplevel;
        npcnix = self.packages.${system}.npcnix;
        install = self.packages.${system}.install;
      };


      devShells = {
        default = pkgs.mkShell {

          buildInputs = [ ] ++ commonArgs.buildInputs;
          nativeBuildInputs = builtins.attrValues
            {
              inherit (pkgs) rust-analyzer cargo-udeps typos cargo rustc rustfmt clippy just nixpkgs-fmt shellcheck rnix-lsp;
            } ++ [
            pkgs.nodePackages.bash-language-server
            # This is required to prevent a mangled bash shell in nix develop
            # see: https://discourse.nixos.org/t/interactive-bash-with-nix-develop-flake/15486
            (pkgs.hiPrio pkgs.bashInteractive)
          ] ++ commonArgs.nativeBuildInputs;
          shellHook = ''
            dot_git="$(git rev-parse --git-common-dir)"
            if [[ ! -d "$dot_git/hooks" ]]; then
                mkdir "$dot_git/hooks"
            fi
            for hook in misc/git-hooks/* ; do ln -sf "$(pwd)/$hook" "$dot_git/hooks/" ; done
            ${pkgs.git}/bin/git config commit.template $(pwd)/misc/git-hooks/commit-template.txt
          '';
        };
      };
    }
  );
}

