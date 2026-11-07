#!/usr/bin/env bash
set -euo pipefail

declare -A selection=(
  [full]="--test-workspace=true"
  [nextest]="--test-workspace=true --test-tool=nextest"
  [stages]="--test-package=rslisp -- --test stages"
  [front]="--test-package=rslisp -- --test stages -- expand_fixtures check_fixtures emit_fixtures"
)

cd pull
exit_code=0
# shellcheck disable=SC2086
cargo mutants --workspace --in-place --no-shuffle --cap-lints=true --baseline=skip \
  --timeout 600 --in-diff "$DIFF" --shard "$SHARD" -vV ${selection[$TESTS]} || exit_code=$?

case "$exit_code" in
  0 | 2 | 3) ;;
  *) exit "$exit_code" ;;
esac
