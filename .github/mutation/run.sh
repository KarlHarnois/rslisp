#!/usr/bin/env bash
set -euo pipefail

fixture_tests=(--test-package=rslisp -- --test stages -- expand_fixtures check_fixtures emit_fixtures)
every_test=(--test-workspace=true)

test_mutants() {
  local timeout=$1
  shift
  local exit_code=0
  cargo mutants --workspace --in-place --no-shuffle --cap-lints=true --baseline=skip \
    --timeout "$timeout" --in-diff "$DIFF" --shard "$SHARD" -vV "$@" || exit_code=$?
  case "$exit_code" in
    0 | 2 | 3) ;;
    *) exit "$exit_code" ;;
  esac
}

cd pull
test_mutants 240 "${fixture_tests[@]}"
test_mutants 600 --iterate "${every_test[@]}"
