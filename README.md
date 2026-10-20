# rslisp

rslisp is a small Lisp that compiles to Rust. The name `rslisp` and the `.rslisp` extension are placeholders until the language has a name.

The syntax has few rules, and the compiler infers types and ownership. Most features live in a prelude written in the language itself. The generated Rust has no hidden clones or allocations. [docs/language.md](docs/language.md) describes the language.

```lisp
(defn main ()
  (println (str "hello" " " "world")))
```

## Set up

The project uses a [Nix](https://nixos.org/download/) flake with [devenv](https://devenv.sh/).

1. Enter the development shell:

   ```sh
   nix develop --no-pure-eval
   ```

   If you use [direnv](https://direnv.net/), run `direnv allow` once instead. The shell then loads when you enter the directory.

2. Run the tests:

   ```sh
   cargo test
   ```

3. Build the compiler:

   ```sh
   nix build
   ```

   The binary is `result/bin/rslisp`.

## Use

Run `rslisp --help` to list the commands. Each command takes a source file:

```sh
rslisp run examples/hello.rslisp
```

## Contribute

Follow [docs/diagnostics.md](docs/diagnostics.md) when you add or change a compiler error.
