{
  description = "rslisp (placeholder name), a minimal Lisp that compiles to Rust";

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

      cratePackage = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
      crateVersion = cratePackage.version;

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

      devShellTools =
        pkgs: defaultShell:
        let
          installedOutputs = map (input: input.out or input) defaultShell.nativeBuildInputs;
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
          }
          check_benchmark_tools() {
            (PATH=$1 && command -v valgrind) >/dev/null || fail "a Linux dev shell has no valgrind"
          }
          check_shell ${pkgs.lib.makeBinPath defaultShell.nativeBuildInputs}
          ${benchmarkToolsCheck}
          touch $out
        '';
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
              pname = "rslisp";
              version = crateVersion;
              src = self;
              cargoExtraArgs = "--locked --package rslisp";
              doCheck = false;
            };
          in
          craneLib.buildPackage (
            buildArgs
            // {
              cargoArtifacts = craneLib.buildDepsOnly (buildArgs // { cargoCheckCommand = "true"; });
              nativeBuildInputs = [ pkgs.makeWrapper ];
              postInstall = ''
                wrapProgram $out/bin/rslisp --prefix PATH : ${emittedRustTools}
              '';
              meta = {
                description = cratePackage.description;
                mainProgram = "rslisp";
                license = pkgs.lib.getLicenseFromSpdxId cratePackage.license;
                platforms = supportedSystems;
              };
            }
          );
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell (
          {
            packages = developmentPackages pkgs;
            RUSTC_WRAPPER = pkgs.writeShellScript "sccache-with-stable-tmpdir" ''
              export TMPDIR="$HOME/.cache/sccache-tmp"
              mkdir -p "$TMPDIR"
              exec ${pkgs.lib.getExe pkgs.sccache} "$@"
            '';
            shellHook = ''
              if [[ -z ''${CI:-} ]]; then
                export CARGO_BUILD_JOBS=''${CARGO_BUILD_JOBS:-2}
              fi
            '';
          }
          // coverageTools pkgs
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
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
      });
    };
}
