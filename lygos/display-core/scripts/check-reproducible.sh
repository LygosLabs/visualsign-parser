#!/usr/bin/env bash
# Proves build-wasm.sh gives the same bytes whatever the checkout path:
# exports the committed tree (git archive HEAD) to <count> different absolute
# paths of different depths under <work-dir>, builds each into its own empty
# target dir, and requires a single sha256 for display_core_bg.wasm that also
# equals the pinned one in the committed WASM-SHA256.
#
# Usage: scripts/check-reproducible.sh <empty-work-dir> [count, default 3]
set -euo pipefail

crate_dir=$(cd "$(dirname "$0")/.." && pwd -P)
repo_dir=$(cd "$crate_dir/../.." && pwd -P)
work=${1:?usage: check-reproducible.sh <empty-work-dir> [count]}
count=${2:-3}
mkdir -p "$work"
work=$(cd "$work" && pwd -P)
if [ -n "$(ls -A "$work")" ]; then
  echo "FAIL: work dir is not empty: $work" >&2
  exit 1
fi
if [ "$count" -lt 2 ]; then
  echo "FAIL: need at least two builds" >&2
  exit 1
fi

commit=$(git -C "$repo_dir" rev-parse HEAD)
hashes=()
for i in $(seq 1 "$count"); do
  checkout="$work/path-$i"
  for _ in $(seq 1 "$i"); do checkout="$checkout/deeper-$i"; done
  mkdir -p "$checkout"
  git -C "$repo_dir" archive "$commit" | tar -x -C "$checkout"
  # --allow-different: this script compares the hashes itself below.
  "$checkout/lygos/display-core/scripts/build-wasm.sh" --allow-different "$work/out-$i" "$work/target-$i" \
    > "$work/build-$i.log" 2>&1 || {
    echo "FAIL: build $i failed, see $work/build-$i.log" >&2
    exit 1
  }
  hash=$(sha256sum "$work/out-$i/display_core_bg.wasm" | cut -d' ' -f1)
  echo "build $i  $checkout  $hash"
  hashes+=("$hash")
done

if [ "$(printf '%s\n' "${hashes[@]}" | sort -u | wc -l)" -ne 1 ]; then
  echo "FAIL: builds differ" >&2
  exit 1
fi
pinned=$(git -C "$repo_dir" show "$commit:lygos/display-core/WASM-SHA256" | cut -d' ' -f1)
if [ "${hashes[0]}" != "$pinned" ]; then
  echo "FAIL: builds agree on ${hashes[0]} but WASM-SHA256 at $commit pins $pinned" >&2
  exit 1
fi
echo "OK: $count builds of $commit from different paths, one sha256 ${hashes[0]}, equal to WASM-SHA256"
# Remove only what this script created (the work dir was empty at the start).
rm -rf "$work"/path-* "$work"/target-* "$work"/out-*
