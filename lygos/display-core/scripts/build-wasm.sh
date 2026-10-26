#!/usr/bin/env bash
# Build display-core for the browser:
#   cargo (wasm32-unknown-unknown, profile wasm-release)
#   -> wasm-bindgen --target web -> wasm-opt -Oz
#
# Needs the pinned Rust toolchain with the wasm32-unknown-unknown target, a
# clang with the WebAssembly backend and llvm-ar on PATH (secp256k1-sys builds
# C code), wasm-bindgen-cli 0.2.106, binaryen's wasm-opt and flock.
#
# Usage: scripts/build-wasm.sh <out-dir> [cargo-target-dir]
# Writes <out-dir>/display_core.js and <out-dir>/display_core_bg.wasm.
#
# Same bytes from any checkout path. Cargo mixes the absolute path of every
# path dependency outside this workspace (the fork's crates under src/) into
# that crate's metadata hash, which changes symbol names and so the order of
# functions in the .wasm. --remap-path-prefix does not reach that hash. So the
# build always reads the sources through one fixed path, BUILD_ROOT, a symlink
# to this checkout held under a lock, and that path, cargo home and the target
# dir are remapped out of the output.
set -euo pipefail

BUILD_ROOT=/tmp/display-core-build/repo
LOCK=/tmp/display-core-build.lock

crate_dir=$(cd "$(dirname "$0")/.." && pwd -P)
repo_dir=$(cd "$crate_dir/../.." && pwd -P)
out_dir=${1:?usage: build-wasm.sh <out-dir> [cargo-target-dir]}
target_dir=${2:-$crate_dir/target}
cargo_home=${CARGO_HOME:-$HOME/.cargo}
mkdir -p "$out_dir" "$target_dir" "$(dirname "$BUILD_ROOT")"
out_dir=$(cd "$out_dir" && pwd -P)
target_dir=$(cd "$target_dir" && pwd -P)

wasm-bindgen --version | grep -qx 'wasm-bindgen 0.2.106' || {
  echo "wasm-bindgen-cli 0.2.106 required" >&2
  exit 1
}

exec 9>"$LOCK"
flock -w 900 9 || {
  echo "another display-core build holds $LOCK" >&2
  exit 1
}
ln -sfn "$repo_dir" "$BUILD_ROOT"

export RUSTFLAGS="--remap-path-prefix=$BUILD_ROOT=/repo --remap-path-prefix=$repo_dir=/repo --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$target_dir=/target"

# The working directory picks the toolchain (rust-toolchain.toml); the
# manifest path, through BUILD_ROOT, picks every source path cargo sees.
cd "$crate_dir"
cargo build --locked --manifest-path "$BUILD_ROOT/lygos/display-core/Cargo.toml" \
  -p display-core --profile wasm-release \
  --target wasm32-unknown-unknown --target-dir "$target_dir"
wasm-bindgen --target web --out-dir "$out_dir" --out-name display_core \
  "$target_dir/wasm32-unknown-unknown/wasm-release/display_core.wasm"
# The features rustc 1.94 enables by default for wasm32-unknown-unknown.
wasm-opt -Oz --enable-bulk-memory --enable-bulk-memory-opt \
  --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals \
  --enable-reference-types --enable-multivalue \
  "$out_dir/display_core_bg.wasm" -o "$out_dir/display_core_bg.opt.wasm"
mv "$out_dir/display_core_bg.opt.wasm" "$out_dir/display_core_bg.wasm"
sha256sum "$out_dir/display_core_bg.wasm"
