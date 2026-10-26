#!/usr/bin/env bash
# Fails unless display-core links exactly the crates listed in
# LINKED-CRATES.txt (name, version, and for non-registry crates their source):
# the visualsign core, its protobuf types (generated), the Bitcoin parser, our
# owned-script crate and their registry dependencies, nothing more and nothing
# less. With a .wasm argument it also fails if another chain's name appears in
# the built module (any case).
#
# Usage: scripts/check-chain-deps.sh [path/to/display_core_bg.wasm]
#        scripts/check-chain-deps.sh --write   (rewrite LINKED-CRATES.txt after
#                                               a reviewed dependency change)
set -euo pipefail

crate_dir=$(cd "$(dirname "$0")/.." && pwd -P)
repo_dir=$(cd "$crate_dir/../.." && pwd -P)
cd "$crate_dir"

# One line per linked crate: "name vX.Y.Z", plus "(path <dir relative to the
# repo>)" for path crates and the git source for git crates.
linked=$(cargo tree --locked -p display-core -e normal --target wasm32-unknown-unknown \
  --prefix none --format '{p}' | sed 's/ (\*)$//' | sed "s| ($repo_dir/| (path |" | sort -u)

if [ "${1:-}" = --write ]; then
  printf '%s\n' "$linked" > LINKED-CRATES.txt
  echo "wrote LINKED-CRATES.txt ($(wc -l < LINKED-CRATES.txt) crates)"
  exit 0
fi

status=0
if ! diff <(cat LINKED-CRATES.txt) <(printf '%s\n' "$linked") > /dev/null; then
  echo "FAIL: display-core links a different set of crates than LINKED-CRATES.txt (< pinned, > now):" >&2
  diff <(cat LINKED-CRATES.txt) <(printf '%s\n' "$linked") | grep '^[<>]' >&2 || true
  status=1
else
  echo "display-core links exactly the $(wc -l < LINKED-CRATES.txt) crates in LINKED-CRATES.txt"
  grep '(path ' LINKED-CRATES.txt | sed 's/^/  path crate: /'
fi

if [ $# -ge 1 ]; then
  # Other chains' names in any case, and the protobuf chain enum names.
  hits=$({
    grep -a -o -i -E 'solana|ethereum|aptos|polkadot|cosmos|sui|tron|near' "$1" || true
    grep -a -o -E 'CHAIN_[A-Z]+' "$1" || true
  } | sort | uniq -c)
  if [ -n "$hits" ]; then
    echo "FAIL: other-chain names inside $1:" >&2
    echo "$hits" >&2
    status=1
  else
    echo "no other-chain names inside $(basename "$1")"
  fi
fi

[ "$status" -eq 0 ] || exit 1
echo "OK: display-core links only the pinned crates (the visualsign core and the Bitcoin parser)"
