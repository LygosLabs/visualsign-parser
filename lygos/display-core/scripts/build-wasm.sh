#!/usr/bin/env bash
# Build display-core for the browser:
#   cargo (wasm32-unknown-unknown, profile wasm-release)
#   -> wasm-bindgen --target web -> wasm-opt -Oz
#
# Usage: scripts/build-wasm.sh [--allow-different] <out-dir> [cargo-target-dir]
# Writes <out-dir>/display_core.js and <out-dir>/display_core_bg.wasm, prints
# its sha256 and whether it equals the pinned one in WASM-SHA256, and exits 1
# when it does not. --allow-different (only for producing a new pin after a
# reviewed source change) prints the same but exits 0.
#
# The output is meant to be the same bytes on any machine that has the exact
# tools below (BUILD.md says where to get them), from any checkout path. So
# this script:
# - refuses unless each tool is the exact version it names, and prints each,
#   and unless the LLVM libraries clang and llvm-ar load have the pinned
#   sha256;
# - refuses if a cargo config file could change the build (config.toml in
#   CARGO_HOME or in any parent directory's .cargo/);
# - runs every tool in a cleared environment (env -i): no RUSTFLAGS,
#   CARGO_ENCODED_RUSTFLAGS, CFLAGS, CC or anything else that changes codegen
#   gets through, only PATH, HOME, CARGO_HOME, RUSTUP_HOME and the flags set
#   here;
# - reads the sources through one fixed path, BUILD_ROOT, a symlink to this
#   checkout (scripts/build-root.sh says who may own its directory and how
#   several users share it). Cargo mixes the absolute path of every path
#   dependency outside this workspace (the fork's crates under src/) into that
#   crate's metadata hash, which changes symbol names and so the order of
#   functions in the .wasm; --remap-path-prefix does not reach that hash. Do
#   not change BUILD_ROOT: it is part of the output.
set -euo pipefail

RUSTC_VERSION='rustc 1.94.0 (4a4ef493e 2026-03-02)'
CLANG_VERSION='Ubuntu clang version 18.1.3 (1ubuntu1)'
LLVM_AR_VERSION='Ubuntu LLVM version 18.1.3'
WASM_BINDGEN_VERSION='wasm-bindgen 0.2.106'
WASM_OPT_VERSION='wasm-opt version 133 (version_133)'
# The shared libraries clang and llvm-ar load (Ubuntu 24.04 libllvm18 and
# libclang-cpp18, 1:18.1.3-1ubuntu1), by sha256 of the file behind the name.
LIBLLVM_SHA256=5c3a63957e28c15cdf1503892074f032d29b72e36a9824a77dfa87e48a0dafb0
LIBCLANG_CPP_SHA256=b9592746f7f8ff1156680fb1fad5faf5486df7a67aca8327cc959463416c6718

allow_different=false
if [ "${1:-}" = --allow-different ]; then
  allow_different=true
  shift
fi

crate_dir=$(cd "$(dirname "$0")/.." && pwd -P)
repo_dir=$(cd "$crate_dir/../.." && pwd -P)
out_dir=${1:?usage: build-wasm.sh <out-dir> [cargo-target-dir]}
target_dir=${2:-$crate_dir/target}
cargo_home=${CARGO_HOME:-$HOME/.cargo}
rustup_home=${RUSTUP_HOME:-$HOME/.rustup}

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# Every tool runs with only these variables.
clean_env=(env -i "PATH=$PATH" "HOME=$HOME" "CARGO_HOME=$cargo_home" "RUSTUP_HOME=$rustup_home")

require_version() { # name, expected first line, command...
  local name=$1 want=$2
  shift 2
  local got
  got=$("${clean_env[@]}" "$@" 2>&1 | head -n 1) || true
  echo "tool $name: $got"
  [ "$got" = "$want" ] || fail "$name must be '$want', found '$got' (see BUILD.md)"
}

cd "$crate_dir"
require_version rustc "$RUSTC_VERSION" rustc --version
require_version clang "$CLANG_VERSION" clang --version
require_version llvm-ar "$LLVM_AR_VERSION" llvm-ar --version
require_version wasm-bindgen "$WASM_BINDGEN_VERSION" wasm-bindgen --version
require_version wasm-opt "$WASM_OPT_VERSION" wasm-opt --version
command -v flock > /dev/null || fail "flock is required"

require_library() { # tool, soname, sha256
  local path got
  path=$("${clean_env[@]}" ldd "$("${clean_env[@]}" sh -c "command -v $1")" |
    awk -v so="$2" '$1 == so { print $3 }')
  [ -n "$path" ] || fail "$1 does not load $2 (see BUILD.md)"
  path=$(readlink -f "$path")
  got=$(sha256sum "$path" | cut -d' ' -f1)
  echo "library $2 for $1: $path $got"
  [ "$got" = "$3" ] || fail "$2 loaded by $1 must have sha256 $3 (see BUILD.md)"
}
require_library clang libLLVM.so.18.1 "$LIBLLVM_SHA256"
require_library clang libclang-cpp.so.18.1 "$LIBCLANG_CPP_SHA256"
require_library llvm-ar libLLVM.so.18.1 "$LIBLLVM_SHA256"

# Cargo reads config files from CARGO_HOME and from every .cargo/ above the
# working directory; any of them can change profiles or flags.
for config in "$cargo_home/config" "$cargo_home/config.toml"; do
  [ ! -e "$config" ] || fail "cargo config file $config can change the build; move it away"
done
dir=$crate_dir
while :; do
  for config in "$dir/.cargo/config" "$dir/.cargo/config.toml"; do
    [ ! -e "$config" ] || fail "cargo config file $config can change the build; move it away"
  done
  [ "$dir" = / ] && break
  dir=$(dirname "$dir")
done

mkdir -p "$out_dir" "$target_dir"
out_dir=$(cd "$out_dir" && pwd -P)
target_dir=$(cd "$target_dir" && pwd -P)

# shellcheck source=build-root.sh
. "$crate_dir/scripts/build-root.sh"
claim_build_root "$repo_dir"

rustflags="--remap-path-prefix=$BUILD_ROOT=/repo --remap-path-prefix=$repo_dir=/repo --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$target_dir=/target"
jobs=${CARGO_BUILD_JOBS:-$(nproc)}

# The working directory picks the toolchain (rust-toolchain.toml); the
# manifest path, through BUILD_ROOT, picks every source path cargo sees.
"${clean_env[@]}" "RUSTFLAGS=$rustflags" \
  cargo build --locked --manifest-path "$BUILD_ROOT/lygos/display-core/Cargo.toml" \
  -p display-core --profile wasm-release --target wasm32-unknown-unknown \
  --target-dir "$target_dir" -j "$jobs"
"${clean_env[@]}" wasm-bindgen --target web --out-dir "$out_dir" --out-name display_core \
  "$target_dir/wasm32-unknown-unknown/wasm-release/display_core.wasm"
# The features rustc 1.94 enables by default for wasm32-unknown-unknown.
"${clean_env[@]}" wasm-opt -Oz --enable-bulk-memory --enable-bulk-memory-opt \
  --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals \
  --enable-reference-types --enable-multivalue \
  "$out_dir/display_core_bg.wasm" -o "$out_dir/display_core_bg.opt.wasm"
mv "$out_dir/display_core_bg.opt.wasm" "$out_dir/display_core_bg.wasm"

built=$(sha256sum "$out_dir/display_core_bg.wasm" | cut -d' ' -f1)
pinned=$(cut -d' ' -f1 "$crate_dir/WASM-SHA256" 2>/dev/null || echo none)
echo "built  $built  $out_dir/display_core_bg.wasm"
echo "pinned $pinned  (WASM-SHA256)"
if [ "$built" = "$pinned" ]; then
  echo "MATCH"
elif $allow_different; then
  echo "DIFFERENT from the pin (allowed by --allow-different)"
else
  echo "DIFFERENT from the pin"
  exit 1
fi
