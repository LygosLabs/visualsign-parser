#!/usr/bin/env bash
# Every check for the fork's Bitcoin display work. Stops with a non-zero exit
# at the first failure; prints "ALL CHECKS PASSED" only when all ran.
#
# Usage: lygos/check-all.sh [wasm-out-dir]
# Needs on PATH: cargo (rustup), node, and for the WebAssembly build a clang
# with the WebAssembly backend, llvm-ar, wasm-bindgen 0.2.106, wasm-opt, flock.
# CHECK_JOBS (default 3) caps cargo's parallel jobs.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd -P)
jobs=${CHECK_JOBS:-3}
out=${1:-$repo/lygos/display-core/target/check-all-pkg}

if [ -n "${REGEN_FIXTURES:-}" ]; then
  echo "FAIL: REGEN_FIXTURES is set; regenerating is a separate step" >&2
  exit 1
fi

step() { printf '\n== %s\n' "$*"; }
upstream=(-p visualsign-bitcoin -p visualsign-bitcoin-owned)

cd "$repo/src"
step "upstream workspace: tests (visualsign-bitcoin, visualsign-bitcoin-owned)"
nice -n 19 cargo test --locked -j "$jobs" "${upstream[@]}"
step "upstream workspace: visualsign-bitcoin tests without default features"
nice -n 19 cargo test --locked -j "$jobs" -p visualsign-bitcoin --no-default-features
step "upstream workspace: clippy -D warnings"
nice -n 19 cargo clippy --locked -j "$jobs" "${upstream[@]}" --all-targets -- -D warnings
nice -n 19 cargo clippy --locked -j "$jobs" -p visualsign-bitcoin --no-default-features \
  --all-targets -- -D warnings
step "upstream workspace: fmt --check"
cargo fmt --check "${upstream[@]}"

cd "$repo/lygos/display-core"
step "display-core: tests"
nice -n 19 cargo test --locked -j "$jobs"
step "display-core: clippy -D warnings"
nice -n 19 cargo clippy --locked -j "$jobs" --all-targets -- -D warnings
step "display-core: fmt --check"
cargo fmt --check

step "isolation: no lowmemory in the upstream workspace"
scripts/check-upstream-isolation.sh

step "WebAssembly build"
CARGO_BUILD_JOBS="$jobs" nice -n 19 scripts/build-wasm.sh "$out"

step "goldens through the WebAssembly build"
node scripts/run-goldens.mjs "$out"

printf '\nALL CHECKS PASSED\n'
