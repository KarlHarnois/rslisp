{
  description = "rslisp (placeholder name), a minimal Lisp that compiles to Rust";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forEachSystem =
        build: nixpkgs.lib.genAttrs supportedSystems (system: build nixpkgs.legacyPackages.${system});

      developmentPackages = pkgs: [
        pkgs.rustc
        pkgs.cargo
        pkgs.clippy
        pkgs.rustfmt
        pkgs.cargo-insta
        pkgs.cargo-llvm-cov
        pkgs.git
        pkgs.nixfmt
      ];

      cratePackage = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;
      crateVersion = cratePackage.version;

      coverageTools = pkgs: {
        LLVM_COV = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov";
        LLVM_PROFDATA = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata";
      };

      qualityGate =
        pkgs:
        pkgs.rustPlatform.buildRustPackage (
          {
            pname = "rslisp-quality-gate";
            version = crateVersion;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.cargo-llvm-cov pkgs.clippy pkgs.rustfmt ];
            buildPhase = ''
              runHook preBuild
              cargo xtask coverage
              runHook postBuild
            '';
            doCheck = false;
            installPhase = ''
              touch $out
            '';
          }
          // coverageTools pkgs
        );

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
          check_shell ${pkgs.lib.makeBinPath defaultShell.nativeBuildInputs}
          touch $out
        '';
    in
    {
      packages = forEachSystem (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "rslisp";
          version = crateVersion;
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          doCheck = false;
          postInstall = ''
            wrapProgram $out/bin/rslisp --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.rustc pkgs.rustfmt ]}
          '';
          meta = {
            description = cratePackage.description;
            mainProgram = "rslisp";
            license = pkgs.lib.getLicenseFromSpdxId cratePackage.license;
            platforms = supportedSystems;
          };
        };
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell (
          {
            packages = developmentPackages pkgs ++ [ pkgs.rust-analyzer ];
          }
          // coverageTools pkgs
        );
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt-tree);

      checks = nixpkgs.lib.genAttrs supportedSystems (system: {
        default = self.packages.${system}.default;
        quality-gate = qualityGate nixpkgs.legacyPackages.${system};
        agent-docs = agentDocs nixpkgs.legacyPackages.${system};
        dev-shell-tools = devShellTools nixpkgs.legacyPackages.${system} self.devShells.${system}.default;
      });
    };
}
