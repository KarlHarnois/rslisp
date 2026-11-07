#!/usr/bin/env bash
set -euo pipefail

targets=$1
build_targets=${targets%% -- *}

timed() {
  local label=$1 started
  shift
  started=$(date +%s.%N)
  "$@"
  printf 'TIMING %s seconds=%.1f\n' "$label" "$(bc <<< "$(date +%s.%N) - $started")"
}

source <(cargo llvm-cov show-env --sh)
timed clean cargo llvm-cov clean --workspace
timed build cargo test --no-run $build_targets
timed test cargo test $targets
timed report cargo llvm-cov report --ignore-filename-regex '/xtask/' --lcov --output-path target/shard.info
