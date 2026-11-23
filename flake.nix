{
  description = "cartograph: a queryable code graph and an interactive map of it";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSystem = build: nixpkgs.lib.genAttrs systems (system: build nixpkgs.legacyPackages.${system});
    in
    {
      packages = forEachSystem (pkgs: rec {
        # The map crate embeds this bundle, so it has to exist before cargo runs.
        map-frontend = pkgs.buildNpmPackage {
          pname = "cartograph-map-frontend";
          version = "0.1.0";
          src = ./crates/map/frontend;
          npmDeps = pkgs.importNpmLock { npmRoot = ./crates/map/frontend; };
          npmConfigHook = pkgs.importNpmLock.npmConfigHook;
          installPhase = "cp -r dist $out";
        };

        cartograph = pkgs.rustPlatform.buildRustPackage {
          pname = "cartograph";
          version = "0.1.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "--package" "cli" ];
          preBuild = ''
            rm -rf crates/map/frontend/dist
            cp -r ${map-frontend} crates/map/frontend/dist
            chmod -R u+w crates/map/frontend/dist
          '';
          # The integration tests start the binary, a server and 30 parallel processes; they run
          # in CI (`cargo test`), not inside the build sandbox.
          doCheck = false;
          meta.mainProgram = "cartograph";
        };

        default = cartograph;
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt nodejs ];
        };
      });
    };
}
