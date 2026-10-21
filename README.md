# rslisp

![line coverage](https://img.shields.io/badge/line%20coverage-97%25-brightgreen)

rslisp is a small Lisp that compiles to Rust. The name `rslisp` and the `.rslisp` extension are placeholders until the language has a name.

The syntax has few rules, and the compiler infers types and ownership. Most features live in a prelude written in the language itself. The generated Rust has no hidden clones or allocations. [docs/language.md](docs/language.md) describes the language.

```lisp
(defn main ()
  (println (str "hello" " " "world")))
```

## Set up

The project uses a [Nix](https://nixos.org/download/) flake.

1. Enter the development shell:

   ```sh
   nix develop
   ```

   If you use [direnv](https://direnv.net/), run `direnv allow` once instead. The shell then loads when you enter the directory.

2. Run the tests with the coverage and CRAP checks:

   ```sh
   cargo xtask coverage
   ```

   `cargo test` runs the tests alone.

3. Build the compiler:

   ```sh
   nix build
   ```

   The binary is `result/bin/rslisp`.

Run `nix fmt` to format the Nix files, and `nix flake check` to run every check that CI runs.

## Use

Run `rslisp --help` to list the commands. Each command takes a source file:

```sh
rslisp run examples/hello.rslisp
rslisp emit examples/geometry/main.rslisp
```

## Contribute

Read [AGENTS.md](AGENTS.md) before you change the code. It holds the rules for this repository and lists the commands a change must pass. Follow [docs/diagnostics.md](docs/diagnostics.md) when you add or change a compiler error.
