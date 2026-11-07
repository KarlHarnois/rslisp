{
  description = "knave, a small Lisp for humans and agents that infers types and ownership and compiles to readable Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSystem =
        build: nixpkgs.lib.genAttrs supportedSystems (system: build nixpkgs.legacyPackages.${system});

      ciPackages =
        pkgs:
        [
          pkgs.rustc
          pkgs.cargo
          pkgs.clippy
          pkgs.rustfmt
          pkgs.cargo-llvm-cov
          pkgs.git
        ]
        ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.valgrind ];

      developmentPackages =
        pkgs:
        ciPackages pkgs
        ++ [
          pkgs.cargo-insta
          pkgs.nixfmt
          pkgs.rust-analyzer
          pkgs.sccache
        ];

      moldLinker =
        pkgs:
        pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
          nativeBuildInputs = [ pkgs.mold ];
          "CARGO_TARGET_${pkgs.stdenv.hostPlatform.rust.cargoEnvVarTarget}_RUSTFLAGS" =
            "-C link-arg=-fuse-ld=mold";
        };

      cargoManifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      cratePackage = cargoManifest.package;
      crateVersion = cratePackage.version;
      installedProfileName = "dist";

      coverageTools = pkgs: {
        LLVM_COV = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov";
        LLVM_PROFDATA = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata";
      };

      agentDocs =
        pkgs:
        pkgs.runCommand "agent-docs" { } ''
          fail() {
            echo "$1" >&2
            exit 1
          }
          guide=${./AGENTS.md}
          references=${./docs}
          budget=25000
          size=$(wc -c < "$guide")
          (( size <= budget )) ||
            fail "AGENTS.md is $size bytes, over its budget of $budget. Move a rule that only one kind of task needs into a reference file under docs/."
          if grep --quiet '^@' "$guide"; then
            fail "AGENTS.md imports a file with @, which loads the file in every session. Name the file in the Reference files table instead."
          fi
          for file in $(cd "$references" && find . -name '*.md' | sed 's#^\./##'); do
            grep --fixed-strings --quiet "| \`docs/$file\` |" "$guide" ||
              fail "The Reference files table in AGENTS.md has no row for docs/$file."
          done
          for file in $(grep --only-matching '^| `docs/[^`]*`' "$guide" | sed 's#^| `docs/##; s#`$##'); do
            [[ -f $references/$file ]] ||
              fail "The Reference files table in AGENTS.md names docs/$file, which does not exist."
          done
          touch $out
        '';

      cargoWithBuildSlots =
        pkgs: realCargo:
        pkgs.writeShellApplication {
          name = "cargo";
          text = ''
            real_cargo=${pkgs.lib.getExe' realCargo "cargo"}
            flock=${pkgs.lib.getExe pkgs.flock}
            slot_directory=$HOME/.cache/knave/build-slots
            slot_limit=''${KNAVE_BUILD_SLOTS:-2}
            seconds_between_attempts=1

            main() {
              if [[ -n ''${CI:-} || -n ''${KNAVE_BUILD_SLOT_HELD:-} ]] || ! command_compiles "$@"; then
                exec "$real_cargo" "$@"
              fi
              require_valid_slot_limit
              mkdir -p "$slot_directory"
              wait_for_slot
              # Keep the lock from cargo: a daemon that a build starts, such as sccache, would inherit it and hold the slot.
              KNAVE_BUILD_SLOT_HELD=1 "$real_cargo" "$@" {slot_lock}>&-
            }

            command_compiles() {
              case $(subcommand "$@") in
                "" | add | clean | config | fetch | fmt | generate-lockfile | help | info | init | locate-project | login | \
                  logout | metadata | new | owner | pkgid | read-manifest | remove | report | search | tree | update | \
                  vendor | verify-project | version | yank) return 1 ;;
              esac
            }

            subcommand() {
              while (($# > 0)); do
                case $1 in
                  --color | --config | --explain | -C | -Z) shift "$(($# > 1 ? 2 : 1))" ;;
                  -*) shift ;;
                  *)
                    echo "$1"
                    return
                    ;;
                esac
              done
            }

            require_valid_slot_limit() {
              if [[ ! $slot_limit =~ ^[1-9][0-9]*$ ]]; then
                echo "KNAVE_BUILD_SLOTS must be a positive whole number, not '$slot_limit'" >&2
                exit 2
              fi
            }

            wait_for_slot() {
              until take_free_slot; do
                if [[ -z ''${told_user_about_wait:-} ]]; then
                  echo "cargo: waiting for one of $slot_limit build slots" >&2
                  told_user_about_wait=yes
                fi
                sleep "$seconds_between_attempts"
              done
            }

            take_free_slot() {
              local slot
              for ((slot = 1; slot <= slot_limit; slot++)); do
                exec {slot_lock}>"$slot_directory/$slot"
                if "$flock" -n "$slot_lock"; then
                  return
                fi
                exec {slot_lock}>&-
              done
              return 1
            }

            main "$@"
          '';
        };

      slotLimitedCargo = pkgs: cargoWithBuildSlots pkgs pkgs.cargo;

      devShellTools =
        pkgs: defaultShell:
        let
          installedOutputs = map (input: input.out or input) defaultShell.nativeBuildInputs;
          slotCargo = slotLimitedCargo pkgs;
          benchmarkToolsCheck = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            check_benchmark_tools ${pkgs.lib.makeBinPath installedOutputs}
          '';
        in
        pkgs.runCommand "dev-shell-tools" { } ''
          fail() {
            echo "$1" >&2
            exit 1
          }
          check_shell() {
            local analyzer pattern="RUST_SRC_PATH-'([^']*)'"
            analyzer=$(PATH=$1 && command -v rust-analyzer) || fail "a dev shell has no rust-analyzer"
            [[ $analyzer == /nix/store/* ]] || fail "a dev shell takes rust-analyzer from $analyzer"
            [[ $(< "$analyzer") =~ $pattern ]] || fail "rust-analyzer sets no RUST_SRC_PATH"
            [[ -f ''${BASH_REMATCH[1]}/core/src/lib.rs ]] || fail "RUST_SRC_PATH holds no core sources"
            ! (PATH=$1 && command -v rustup) || fail "a dev shell provides rustup"
            [[ $(PATH=$1 && command -v cargo) == ${slotCargo}/bin/cargo ]] ||
              fail "a dev shell runs cargo outside its build slots"
          }
          check_benchmark_tools() {
            (PATH=$1 && command -v valgrind) >/dev/null || fail "a Linux dev shell has no valgrind"
          }
          check_shell ${pkgs.lib.makeBinPath defaultShell.nativeBuildInputs}
          ${benchmarkToolsCheck}
          touch $out
        '';

      buildSlots =
        pkgs:
        let
          recordingCargo = pkgs.writeShellScriptBin "cargo" ''
            touch "$calls/$1-''${2:-}"
            case $1 in
              build)
                until [[ -e $calls/finish-$2 ]]; do sleep 0.1; done
                ;;
              failing) exit 3 ;;
              xtask) timeout 5 "$slot_limited_cargo" check inside-xtask ;;
              daemon)
                sleep 300 >/dev/null 2>&1 &
                echo $! >"$calls/daemon-process"
                ;;
            esac
          '';
          cargo = pkgs.lib.getExe' (cargoWithBuildSlots pkgs recordingCargo) "cargo";
        in
        pkgs.runCommand "build-slots" { } ''
          fail() {
            echo "$1" >&2
            exit 1
          }
          wait_for_call() {
            for _ in {1..100}; do
              [[ -e $calls/$1 ]] && return
              sleep 0.1
            done
            fail "cargo $1 never ran"
          }
          expect_no_call() {
            sleep 2
            [[ ! -e $calls/$1 ]] || fail "cargo $1 ran with no free slot"
          }
          finish() {
            touch "$calls/finish-$1"
          }
          export HOME=$PWD/home calls=$PWD/calls slot_limited_cargo=${cargo}
          unset CI KNAVE_BUILD_SLOTS KNAVE_BUILD_SLOT_HELD
          mkdir -p "$HOME" "$calls"

          ${cargo} build first &
          ${cargo} build second &
          wait_for_call build-first
          wait_for_call build-second

          ${cargo} build third 2>waiting &
          third=$!
          expect_no_call build-third
          grep --quiet 'waiting for one of 2 build slots' waiting || fail "a waiting command says nothing"

          ${cargo} check --color always 2>/dev/null &
          blocked_check=$!
          expect_no_call check---color

          timeout 5 ${cargo} fmt --all || fail "fmt waited for a slot"
          timeout 5 ${cargo} --version || fail "--version waited for a slot"
          timeout 5 ${cargo} --color always metadata || fail "metadata waited for a slot"
          timeout 5 ${cargo} --color || fail "a flag with no value waited for a slot"
          timeout 5 env KNAVE_BUILD_SLOT_HELD=1 ${cargo} check nested || fail "a nested cargo waited for a slot"
          timeout 5 env CI=true ${cargo} check ci || fail "cargo waited for a slot under CI"
          timeout 5 env KNAVE_BUILD_SLOTS=3 ${cargo} check third-slot || fail "KNAVE_BUILD_SLOTS did not raise the limit"

          limit_status=0
          KNAVE_BUILD_SLOTS=0 timeout 5 ${cargo} check 2>/dev/null || limit_status=$?
          [[ $limit_status == 2 ]] || fail "a slot limit of 0 was not refused"
          finish first
          wait_for_call build-third
          finish third
          wait "$third"
          wait "$blocked_check"

          ${cargo} build killed &
          killed=$!
          wait_for_call build-killed
          ${cargo} build after-kill 2>/dev/null &
          kill -KILL "$killed"
          wait_for_call build-after-kill
          finish killed
          finish second
          finish after-kill
          wait

          status=0
          ${cargo} failing || status=$?
          [[ $status == 3 ]] || fail "cargo exit status $status was not passed on"

          KNAVE_BUILD_SLOTS=1 ${cargo} xtask || fail "a cargo inside a slot waited for a second slot"

          KNAVE_BUILD_SLOTS=1 ${cargo} daemon
          timeout 5 env KNAVE_BUILD_SLOTS=1 ${cargo} check after-daemon || fail "a process that outlived cargo kept the slot"
          kill "$(<"$calls/daemon-process")"
          touch $out
        '';

      sourceFormat =
        pkgs: package:
        pkgs.runCommand "source-format" { } ''
          fixtures=${./tests/fixtures}
          mapfile -t fixture_files < <(
            find "$fixtures" -name '*.rslisp' \
              -not -path "$fixtures/read/*" \
              -not -path "$fixtures/format/*" \
              -not -path "$fixtures/expand/unreadable/*" | sort
          )
          ${pkgs.lib.getExe package} fmt --check ${./prelude} ${./examples} "''${fixture_files[@]}" ||
            { echo "run knave fmt on the files above to lay them out" >&2; exit 1; }
          touch $out
        '';

      installedProfile =
        pkgs: package:
        let
          wantedSettings = {
            inherits = "release";
            codegen-units = 1;
            lto = "fat";
          };
          declaredSettings = cargoManifest.profile.${installedProfileName} or { };
          driftedSettings = pkgs.lib.filterAttrs (
            setting: value: declaredSettings.${setting} or null != value
          ) wantedSettings;
          renderedSettings = pkgs.lib.concatMapAttrsStringSep ", " (
            setting: value: "${setting} = ${builtins.toJSON value}"
          ) wantedSettings;
          builds = [
            package
            package.cargoArtifacts
          ];
          buildsOnOtherProfiles = builtins.filter (
            build: build.CARGO_PROFILE or null != installedProfileName
          ) builds;
          namesOfBuildsOnOtherProfiles = map (build: build.name) buildsOnOtherProfiles;
          profileKeepsSettings = driftedSettings == { };
          everyBuildUsesProfile = buildsOnOtherProfiles == [ ];
        in
        assert pkgs.lib.assertMsg profileKeepsSettings
          "[profile.${installedProfileName}] in Cargo.toml must set ${renderedSettings}.";
        assert pkgs.lib.assertMsg everyBuildUsesProfile
          "${pkgs.lib.concatStringsSep ", " namesOfBuildsOnOtherProfiles} must set CARGO_PROFILE = \"${installedProfileName}\".";
        pkgs.runCommand "installed-profile" { } "touch $out";
    in
    {
      packages = forEachSystem (pkgs: {
        default =
          let
            craneLib = crane.mkLib pkgs;
            emittedRustTools = pkgs.lib.makeBinPath [
              pkgs.rustc
              pkgs.rustfmt
            ];
            buildArgs = {
              pname = "knave";
              version = crateVersion;
              src = self;
              cargoExtraArgs = "--locked --package knave";
              CARGO_PROFILE = installedProfileName;
              doCheck = false;
            };
          in
          craneLib.buildPackage (
            buildArgs
            // {
              cargoArtifacts = craneLib.buildDepsOnly (buildArgs // { cargoCheckCommand = "true"; });
              nativeBuildInputs = [ pkgs.makeWrapper ];
              postInstall = ''
                wrapProgram $out/bin/knave --prefix PATH : ${emittedRustTools}
              '';
              meta = {
                description = cratePackage.description;
                mainProgram = "knave";
                license = pkgs.lib.getLicenseFromSpdxId cratePackage.license;
                platforms = supportedSystems;
              };
            }
          );
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell (
          {
            packages = [ (slotLimitedCargo pkgs) ] ++ developmentPackages pkgs;
            RUSTC_WRAPPER = pkgs.writeShellScript "sccache-with-stable-tmpdir" ''
              export TMPDIR="$HOME/.cache/sccache-tmp"
              mkdir -p "$TMPDIR"
              exec ${pkgs.lib.getExe pkgs.sccache} "$@"
            '';
            shellHook = ''
              if [[ -z ''${CI:-} ]]; then
                export CARGO_BUILD_JOBS=''${CARGO_BUILD_JOBS:-2}
                export CARGO_INCREMENTAL=0
              fi
            '';
          }
          // coverageTools pkgs
          // moldLinker pkgs
        );
        ci = pkgs.mkShell (
          {
            packages = ciPackages pkgs;
          }
          // coverageTools pkgs
        );
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt-tree);

      checks = nixpkgs.lib.genAttrs supportedSystems (system: {
        default = self.packages.${system}.default;
        agent-docs = agentDocs nixpkgs.legacyPackages.${system};
        build-slots = buildSlots nixpkgs.legacyPackages.${system};
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
        installed-profile =
          installedProfile nixpkgs.legacyPackages.${system}
            self.packages.${system}.default;
        source-format = sourceFormat nixpkgs.legacyPackages.${system} self.packages.${system}.default;
      });
    };
}
