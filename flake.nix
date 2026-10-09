# This flake file is community maintained
{
  description = "Niri Charm: niri with procedural knitted borders.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      revision = self.shortRev or self.dirtyShortRev or "unknown";
      niri-charm-package =
        {
          lib,
          cairo,
          dbus,
          libGL,
          libdisplay-info_0_3,
          libinput,
          seatd,
          libxkbcommon,
          libgbm,
          pango,
          nasm,
          pipewire,
          pkg-config,
          rustPlatform,
          systemd,
          wayland,
          withDbus ? true,
          withSystemd ? true,
          withScreencastSupport ? true,
          withDinit ? false,
        }:

        rustPlatform.buildRustPackage {
          pname = "niri-charm";
          version = revision;

          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./niri-config
              ./niri-ipc
              ./niri-visual-tests
              ./resources
              ./src
              ./Cargo.toml
              ./Cargo.lock
              ./build.rs
            ];
          };

          postPatch = ''
            patchShebangs resources/niri-charm/niri-charm-session
            substituteInPlace resources/niri-charm/niri-charm-session \
              --replace-fail '/usr/local/bin/niri-charm' "$out/bin/niri-charm" \
              --replace-fail '/usr/local/share/niri-charm/default-config.kdl' "$out/share/niri-charm/default-config.kdl"
            substituteInPlace resources/niri-charm/niri-charm.service \
              --replace-fail '/usr/local/bin/niri-charm-session' "$out/bin/niri-charm-session"
            substituteInPlace resources/niri-charm/niri-charm.desktop \
              --replace-fail '/usr/local/bin/niri-charm-session' "$out/bin/niri-charm-session"
          '';

          cargoLock = {
            # NOTE: This is only used for Git dependencies
            allowBuiltinFetchGit = true;
            lockFile = ./Cargo.lock;
          };

          strictDeps = true;

          nativeBuildInputs = [
            rustPlatform.bindgenHook
            pkg-config
            nasm
          ];

          buildInputs = [
            cairo
            dbus
            libGL
            libdisplay-info_0_3
            libinput
            seatd
            libxkbcommon
            libgbm
            pango
            wayland
          ]
          ++ lib.optional (withDbus || withScreencastSupport || withSystemd) dbus
          ++ lib.optional withScreencastSupport pipewire
          # Also includes libudev
          ++ lib.optional withSystemd systemd;

          buildFeatures =
            lib.optional withDbus "dbus"
            ++ lib.optional withDinit "dinit"
            ++ lib.optional withScreencastSupport "xdp-gnome-screencast"
            ++ lib.optional withSystemd "systemd";
          buildNoDefaultFeatures = true;

          # ever since this commit:
          # https://github.com/niri-wm/niri/commit/771ea1e81557ffe7af9cbdbec161601575b64d81
          # niri now runs an actual instance of the real compositor (with a mock backend) during tests
          # and thus creates a real socket file in the runtime dir.
          # this is fine for our build, we just need to make sure it has a directory to write to.
          preCheck = ''
            export XDG_RUNTIME_DIR="$(mktemp -d)"
          '';

          checkFlags = [
            # These tests require the ability to access a "valid EGL Display", but that won't work
            # inside the Nix sandbox
            "--skip=::egl"
          ];
          postInstall = ''
            mv $out/bin/niri $out/bin/niri-charm

            install -Dm644 resources/niri-charm/niri-charm.desktop -t $out/share/wayland-sessions
            install -Dm644 resources/niri-portals.conf $out/share/xdg-desktop-portal/niri-charm-portals.conf
            install -Dm644 resources/default-config.kdl $out/share/niri-charm/default-config.kdl
          ''
          + lib.optionalString withSystemd ''
            install -Dm755 resources/niri-charm/niri-charm-session $out/bin/niri-charm-session
            install -Dm644 \
              resources/niri-charm/niri-charm.service \
              resources/niri-charm/niri-charm-shutdown.target \
              -t $out/lib/systemd/user
          '';

          env = {
            # Force linking with libEGL and libwayland-client so they end up in RPATH and
            # can be discovered by `dlopen()`
            RUSTFLAGS = toString (
              map (arg: "-C link-arg=" + arg) [
                "-Wl,--push-state,--no-as-needed"
                "-lEGL"
                "-lwayland-client"
                "-Wl,--pop-state"
              ]
            );
            NIRI_BUILD_COMMIT = revision;
          };

          passthru = {
            providedSessions = [ "niri-charm" ];
          };

          meta = {
            description = "Scrollable-tiling Wayland compositor with procedural knitted borders";
            license = lib.licenses.gpl3Only;
            mainProgram = "niri-charm";
            platforms = lib.platforms.linux;
          };
        };

      inherit (nixpkgs) lib;
      # Support all Linux systems that the nixpkgs flake exposes
      systems = lib.intersectLists lib.systems.flakeExposed lib.platforms.linux;

      forAllSystems = lib.genAttrs systems;
      nixpkgsFor = forAllSystems (system: nixpkgs.legacyPackages.${system});
    in
    {
      checks = forAllSystems (system: {
        # We use the debug build here to save a bit of time.
        niri-charm-debug = self.packages.${system}.niri-charm-debug;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgsFor.${system};
          rustfmt' = pkgs.rustfmt.override { asNightly = true; };
          niriCharm = self.packages.${system}.niri-charm;
        in
        {
          default = pkgs.mkShell {
            packages = builtins.attrValues {
              inherit (pkgs)
                rustc
                cargo
                clippy
                cargo-insta
                ;
              inherit rustfmt';
            };

            nativeBuildInputs = [
              pkgs.rustPlatform.bindgenHook
              pkgs.pkg-config
              pkgs.nasm
              pkgs.wrapGAppsHook4 # For `niri-visual-tests`
            ];

            buildInputs = niriCharm.buildInputs ++ [
              pkgs.libadwaita # For `niri-visual-tests`
            ];

            env = {
              # WARN: Do not overwrite this variable in your shell!
              # It is required for `dlopen()` to work on some libraries; see the comment
              # in the package expression.
              #
              # This should only be set with `RUSTFLAGS="$RUSTFLAGS -C your-flags"`.
              RUSTFLAGS = niriCharm.RUSTFLAGS;
            };
          };
        }
      );

      formatter = forAllSystems (system: nixpkgsFor.${system}.nixfmt-rfc-style);

      packages = forAllSystems (
        system:
        let
          niriCharm = nixpkgsFor.${system}.callPackage niri-charm-package { };
        in
        {
          niri-charm = niriCharm;

          # NOTE: This is for development purposes only.
          niri-charm-debug = niriCharm.overrideAttrs (
            newAttrs: oldAttrs: {
              pname = oldAttrs.pname + "-debug";

              cargoBuildType = "debug";
              cargoCheckType = newAttrs.cargoBuildType;

              dontStrip = true;
            }
          );

          default = niriCharm;
        }
      );

      overlays.default = final: _: {
        niri-charm = final.callPackage niri-charm-package { };
      };

      nixosModules.default =
        { pkgs, ... }:
        let
          package = pkgs.callPackage niri-charm-package { };
        in
        {
          environment.systemPackages = [ package ];
          services.displayManager.sessionPackages = [ package ];
        };
    };
}
