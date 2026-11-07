#!/usr/bin/env bash
set -euo pipefail

moved=src/ownership/uncoerced_parameters.rs
renamed=src/ownership/relocated_parameters.rs
differences=$(mktemp -d)

list_mutants_of() {
  echo "== $1"
  cargo mutants --list --workspace --in-diff "$differences/$1" || echo "exit code $?"
}

sed -n '29,34p' "$moved" > "$differences/block"
sed -i '29,34d' "$moved"
sed -i "39r $differences/block" "$moved"
git diff > "$differences/moved-within-file"
list_mutants_of moved-within-file
git checkout -- .

git mv "$moved" "$renamed"
sed -i 's/uncoerced_parameters/relocated_parameters/' src/ownership/mod.rs
git diff HEAD > "$differences/renamed-file"
git diff HEAD --no-renames > "$differences/renamed-file-without-detection"
list_mutants_of renamed-file
list_mutants_of renamed-file-without-detection
