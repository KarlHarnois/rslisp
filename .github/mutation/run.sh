#!/usr/bin/env bash
set -euo pipefail

case "$TESTS" in
  full) selection=(--test-workspace=true) ;;
  nextest) selection=(--test-workspace=true --test-tool=nextest) ;;
  front) selection=(--test-package=rslisp -- --test stages -- expand_fixtures check_fixtures emit_fixtures) ;;
esac

cd pull
exit_code=0
cargo mutants --workspace --in-place --no-shuffle --cap-lints=true --baseline=skip \
  --timeout 600 --in-diff "$DIFF" --shard "$SHARD" -vV "${selection[@]}" || exit_code=$?

case "$exit_code" in
  0 | 2 | 3) ;;
  *) exit "$exit_code" ;;
esac
