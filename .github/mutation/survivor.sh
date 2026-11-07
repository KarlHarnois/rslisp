#!/usr/bin/env bash
set -uo pipefail

probe=.github/mutation/held_behind_reference.rslisp

report() {
  echo "== $1"
  cargo run --quiet -- emit "$probe"
  cargo run --quiet -- run "$probe"
  echo "exit code $?"
}

report unchanged
sed -i 's/if \*\*referent == \*held => false/if true => false/' src/types/ty.rs
git diff --stat
report guard-always-true
