#!/usr/bin/env bash
# Fails if the upstream workspace (src/) would build anything with secp256k1's
# "lowmemory" feature. display-core turns that feature on for itself; it lives
# in its own workspace so the feature never reaches upstream crates or
# `make -C src build`.
set -euo pipefail

repo_dir=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$repo_dir/src"

if grep -q "display-core" Cargo.toml; then
  echo "FAIL: display-core is named in the upstream workspace (src/Cargo.toml)" >&2
  exit 1
fi
tree=$(cargo tree --locked --workspace -e features --target all)
if grep -q lowmemory <<<"$tree"; then
  echo "FAIL: the upstream workspace enables lowmemory:" >&2
  grep lowmemory <<<"$tree" >&2
  exit 1
fi
echo "OK: upstream workspace has no display-core member and no lowmemory feature"
