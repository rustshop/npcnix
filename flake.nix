{
  description = "dpc's basic flake template";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?rev=f294325aed382b66c7a188482101b0f336d1d7db"; # nixos-unstable
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane?rev=445a3d222947632b5593112bb817850e8a9cf737"; # v0.12.1
    crane.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, crane }:
    flake-utils.lib.eachDefaultSystem (system:
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

            buildInputs = with pkgs; [
              openssl
              pkg-config
            ];

            nativeBuildInputs = [
            ];
          };
        npcnixPkgUnwrapped = craneLib.buildPackage ({ } // commonArgs);
        npcnixPkgWrapped = pkgs.writeShellScriptBin "npcnix" ''
          exec env \
            NPCNIX_AWS_CLI=''${NPCNIX_AWS_CLI:-${pkgs.awscli2}/bin/aws} \
            NPCNIX_NIXOS_REBUILD=''${NPCNIX_NIXOS_REBUILD:-${pkgs.nixos-rebuild}/bin/nixos-rebuild} \
            ${npcnixPkgUnwrapped}/bin/npcnix "$@"
        '';
      in
      {
        packages =
          {
            default = npcnixPkgWrapped;
            npcnix-unwrapped = npcnixPkgUnwrapped;
            npcnix = npcnixPkgWrapped;
            install = pkgs.writeShellScriptBin "npcnix-install" ''
              set -e
              npcnix_swapfile="/npcnix-swapfile"

              function cleanup() {
                ${pkgs.util-linux}/bin/swapoff "$npcnix_swapfile" || true
                ${pkgs.coreutils}/bin/rm -f "$npcnix_swapfile" || true
              }
              if [ ! -e "$npcnix_swapfile" ]; then
                # it has been experimentally verified, that 2G should be enough
                # bootstrap even on AWS EC2 t3.nano instances
                ${pkgs.util-linux}/bin/fallocate -l 2G "$npcnix_swapfile" || true
              fi
              trap cleanup EXIT
              chmod 600 "$npcnix_swapfile" && \
                ${pkgs.util-linux}/bin/mkswap "$npcnix_swapfile" && \
                ${pkgs.util-linux}/bin/swapon "$npcnix_swapfile" || \
                true

              ${npcnixPkgWrapped}/bin/npcnix install "$@"
            '';
          };


        devShells = {
          default = pkgs.mkShell {

            buildInputs = [ ] ++ commonArgs.buildInputs;
            nativeBuildInputs = with pkgs; [
              rust-analyzer
              cargo-udeps
              typos
              cargo
              rustc
              rustfmt
              clippy

              # This is required to prevent a mangled bash shell in nix develop
              # see: https://discourse.nixos.org/t/interactive-bash-with-nix-develop-flake/15486
              (hiPrio pkgs.bashInteractive)

              # Nix
              pkgs.nixpkgs-fmt
              pkgs.shellcheck
              pkgs.rnix-lsp
              pkgs.nodePackages.bash-language-server

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

