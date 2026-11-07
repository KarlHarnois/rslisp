#!/usr/bin/env bash
set -uo pipefail

timed() {
  local label=$1 started
  shift
  started=$(date +%s.%N)
  "$@" > /dev/null 2>&1
  local status=$?
  printf 'TIMING %s status=%s seconds=%.1f\n' "$label" "$status" "$(bc <<< "$(date +%s.%N) - $started")"
}

each_stage_test() {
  cargo test --test stages -- --list 2> /dev/null | sed -n 's/: test$//p'
}

[[ ${1:-} == instrumented ]] && source <(cargo llvm-cov show-env --sh)

timed build cargo test --workspace --no-run
for test_name in $(each_stage_test); do
  timed "stages::$test_name" cargo test --test stages -- --exact "$test_name"
done
timed stages-together cargo test --test stages
timed cli cargo test --test cli
timed lib cargo test --lib
timed expand-lib cargo test -p rslisp-expand --lib
timed xtask cargo test -p xtask
timed diagnostics cargo test --test diagnostic_codes --test diagnostic_snapshots
