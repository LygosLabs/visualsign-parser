#!/usr/bin/env bash
# Build display-core for the browser:
#   cargo (wasm32-unknown-unknown, profile wasm-release)
#   -> wasm-bindgen --target web -> wasm-opt -Oz
#
# Needs the pinned Rust toolchain with the wasm32-unknown-unknown target, a
# clang with the WebAssembly backend and llvm-ar on PATH (secp256k1-sys builds
# C code), wasm-bindgen-cli 0.2.106 and binaryen's wasm-opt.
#
# Usage: scripts/build-wasm.sh <out-dir> [cargo-target-dir]
# Writes <out-dir>/display_core.js and <out-dir>/display_core_bg.wasm.
# Paths are remapped so the output does not depend on where it was built.
set -euo pipefail

crate_dir=$(cd "$(dirname "$0")/.." && pwd)
repo_dir=$(cd "$crate_dir/../.." && pwd)
out_dir=${1:?usage: build-wasm.sh <out-dir> [cargo-target-dir]}
target_dir=${2:-$crate_dir/target}
cargo_home=${CARGO_HOME:-$HOME/.cargo}
mkdir -p "$out_dir" "$target_dir"
target_dir=$(cd "$target_dir" && pwd)

export RUSTFLAGS="--remap-path-prefix=$repo_dir=/repo --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$target_dir=/target"

wasm-bindgen --version | grep -qx 'wasm-bindgen 0.2.106' || {
  echo "wasm-bindgen-cli 0.2.106 required" >&2
  exit 1
}

cd "$crate_dir"
cargo build --locked -p display-core --profile wasm-release \
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
