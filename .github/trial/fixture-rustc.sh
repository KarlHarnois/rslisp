#!/usr/bin/env bash
set -uo pipefail

REAL_RUSTC=$(command -v rustc)
export REAL_RUSTC RUSTC=$REAL_RUSTC
shim_directory=$(mktemp -d)
cat > "$shim_directory/rustc" <<'SHIM'
#!/usr/bin/env bash
exec "$REAL_RUSTC" "$@" $EXTRA_RUSTC_FLAGS
SHIM
chmod +x "$shim_directory/rustc"
PATH=$MOLD_DIRECTORY:$PATH

timed_run_fixtures() {
  local label=$1 started
  export EXTRA_RUSTC_FLAGS=$2
  started=$(date +%s.%N)
  PATH=$shim_directory:$PATH cargo test --test stages -- --exact run_fixtures > /dev/null 2>&1
  local status=$?
  printf 'TIMING %s status=%s seconds=%.1f\n' "$label" "$status" "$(bc <<< "$(date +%s.%N) - $started")"
}

cargo test --test stages --no-run
timed_run_fixtures default ""
timed_run_fixtures mold "-C link-arg=-fuse-ld=mold"
timed_run_fixtures one-codegen-unit "-C codegen-units=1"
timed_run_fixtures mold-one-codegen-unit "-C link-arg=-fuse-ld=mold -C codegen-units=1"
timed_run_fixtures default-again ""
