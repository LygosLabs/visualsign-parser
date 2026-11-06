#!/usr/bin/env bash
# Every check for the fork's Bitcoin display work. Stops with a non-zero exit
# at the first failure; prints "ALL CHECKS PASSED" only when all ran.
#
# Usage: lygos/check-all.sh [work-dir]
# The work dir (default lygos/display-core/target/check-all) holds the wasm
# build and, under repro.*, the logs of the reproducibility builds (the builds
# themselves are removed on success).
# Needs on PATH: cargo (rustup), node, and the exact build tools listed in
# lygos/display-core/BUILD.md. CHECK_JOBS (default 3) caps cargo's jobs.
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd -P)
jobs=${CHECK_JOBS:-3}
work=${1:-$repo/lygos/display-core/target/check-all}

# Pinned test counts per crate (all test targets and doc tests together).
BITCOIN_TESTS=70
# The one upstream test that needs a running bitcoind (tests/regtest.rs regtest_core_psbts_match_parser) is
# ignored here and run by its own regtest lane; exactly that test may be ignored, no other.
BITCOIN_IGNORED=regtest_core_psbts_match_parser
OWNED_TESTS=29
DISPLAY_CORE_TESTS=12

fail() {
  echo "FAIL: $*" >&2
  exit 1
}
step() { printf '\n== %s\n' "$*"; }

if [ -n "${REGEN_FIXTURES:-}" ]; then
  fail "REGEN_FIXTURES is set; regenerating is a separate step"
fi

# cargo test with the exact number of tests pinned and none ignored or
# filtered out.
counted_test() { # label, expected count, allowed ignored test name ("" for none), cargo test args...
  local label=$1 want=$2 allowed_ignored=$3
  shift 3
  local log
  log=$(mktemp)
  nice -n 19 cargo test --locked -j "$jobs" "$@" 2>&1 | tee "$log"
  [ "${PIPESTATUS[0]}" -eq 0 ] || fail "$label: tests failed"
  local passed ignored filtered
  read -r passed ignored filtered < <(awk '/^test result:/ {
      for (i = 1; i < NF; i++) {
        if ($(i + 1) ~ /^passed/) p += $i
        if ($(i + 1) ~ /^ignored/) g += $i
        if ($(i + 1) ~ /^filtered/) f += $i
      }
    } END { print p + 0, g + 0, f + 0 }' "$log")
  grep -E '^test .* \.\.\. ignored' "$log" > "$log.ignored" || true
  rm -f "$log"
  echo "$label: $passed passed, $ignored ignored, $filtered filtered out (pinned $want)"
  if [ -z "$allowed_ignored" ]; then
    [ "$ignored" -eq 0 ] || fail "$label: $ignored ignored tests"
  else
    [ "$ignored" -eq 1 ] || fail "$label: $ignored ignored tests, only $allowed_ignored may be"
    grep -qE "^test $allowed_ignored \.\.\. ignored" "$log.ignored" || fail "$label: the ignored test is not $allowed_ignored"
  fi
  [ "$filtered" -eq 0 ] || fail "$label: $filtered tests filtered out"
  [ "$passed" -eq "$want" ] || fail "$label: $passed tests passed, pinned $want"
  rm -f "$log.ignored"
}

step "fork patches: lygos/FORK-PATCHES.json matches git log"
node "$repo/lygos/check-fork-patches.mjs"

cd "$repo/src"
step "upstream workspace: visualsign-bitcoin tests"
counted_test visualsign-bitcoin "$BITCOIN_TESTS" "$BITCOIN_IGNORED" -p visualsign-bitcoin
step "upstream workspace: visualsign-bitcoin tests without default features"
counted_test "visualsign-bitcoin (no default features)" "$BITCOIN_TESTS" "$BITCOIN_IGNORED" \
  -p visualsign-bitcoin --no-default-features
step "upstream workspace: visualsign-bitcoin-owned tests"
counted_test visualsign-bitcoin-owned "$OWNED_TESTS" "" -p visualsign-bitcoin-owned
step "upstream workspace: clippy -D warnings"
nice -n 19 cargo clippy --locked -j "$jobs" -p visualsign-bitcoin -p visualsign-bitcoin-owned \
  --all-targets -- -D warnings
nice -n 19 cargo clippy --locked -j "$jobs" -p visualsign-bitcoin --no-default-features \
  --all-targets -- -D warnings
step "upstream workspace: fmt --check"
cargo fmt --check -p visualsign-bitcoin -p visualsign-bitcoin-owned

cd "$repo/lygos/display-core"
step "display-core: tests"
counted_test display-core "$DISPLAY_CORE_TESTS" ""
step "display-core: clippy -D warnings"
nice -n 19 cargo clippy --locked -j "$jobs" --all-targets -- -D warnings
step "display-core: fmt --check"
cargo fmt --check

step "isolation: no lowmemory in the upstream workspace"
scripts/check-upstream-isolation.sh

step "WebAssembly build, compared with WASM-SHA256"
mkdir -p "$work"
# build-wasm.sh itself exits 1 when the hash differs from WASM-SHA256; the
# compare below is a second, independent check.
CARGO_BUILD_JOBS="$jobs" nice -n 19 scripts/build-wasm.sh "$work/pkg"
built=$(sha256sum "$work/pkg/display_core_bg.wasm" | cut -d' ' -f1)
pinned=$(cut -d' ' -f1 WASM-SHA256)
[ "$built" = "$pinned" ] || fail "built $built, WASM-SHA256 pins $pinned"
echo "wasm $built equals WASM-SHA256"

step "display-core links exactly LINKED-CRATES.txt, no other chain"
scripts/check-chain-deps.sh "$work/pkg/display_core_bg.wasm"

step "goldens through the WebAssembly build"
node scripts/run-goldens.mjs "$work/pkg"

step "reproducible from three checkout paths (committed tree)"
repro=$(mktemp -d "$work/repro.XXXXXX")
CARGO_BUILD_JOBS="$jobs" nice -n 19 scripts/check-reproducible.sh "$repro" 3

printf '\nALL CHECKS PASSED\n'
