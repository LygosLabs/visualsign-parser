# Building and verifying display-core

`WASM-SHA256` pins the sha256 of `display_core_bg.wasm` built from this
commit. Anyone with the tools below can rebuild it and must get exactly that
hash. Never re-pin a different hash to make a check pass: a mismatch means
the tools, the environment or the source differ, and that must be explained.

## What you need

Tested on x86_64 Linux, Ubuntu 24.04. Other hosts (macOS, aarch64) are not
tested; they may give other bytes.

| Tool | Exact version (first line of `--version`) | Where to get it |
|---|---|---|
| Rust | `rustc 1.94.0 (4a4ef493e 2026-03-02)` | rustup (https://rustup.rs). `rust-toolchain.toml` here selects 1.94.0 and the `wasm32-unknown-unknown` target; rustup installs both on first use. |
| clang | `Ubuntu clang version 18.1.3 (1ubuntu1)` | Ubuntu 24.04 packages `clang-18` and `llvm-18`, version `1:18.1.3-1ubuntu1` (`apt-get install clang-18=1:18.1.3-1ubuntu1 llvm-18=1:18.1.3-1ubuntu1`). Put `/usr/lib/llvm-18/bin` on `PATH` so `clang` and `llvm-ar` resolve to them. Package sha256: clang-18 `628b16701014ef7ad648380b20ea74b90dd543857f933b3e34d2fc042783de25`, llvm-18 `139cb82e16e75fcdd4a56562804ff9bfb482b65d0929580d621d28088075a27e`, libclang-common-18-dev `d41631bb752815a8196261d1030c096591c59678c8adfbf2e6a740a691b49e80`. |
| llvm-ar | `Ubuntu LLVM version 18.1.3` | from `llvm-18` above |
| wasm-bindgen | `wasm-bindgen 0.2.106` | `cargo install wasm-bindgen-cli --version 0.2.106 --locked` |
| wasm-opt | `wasm-opt version 133 (version_133)` | binaryen release `version_133`, `binaryen-version_133-x86_64-linux.tar.gz` from https://github.com/WebAssembly/binaryen/releases/tag/version_133, sha256 `2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489` (also published beside it as `.sha256`). Put its `bin/` on `PATH`. |
| flock, git, sha256sum | any | util-linux, git, coreutils |
| Node.js | 22 (only for the golden replay) | https://nodejs.org |

The build also needs network access the first time (crates.io, and
github.com for the `tkhq/qos` git dependency of the upstream workspace).

`scripts/build-wasm.sh` refuses to build unless each tool prints exactly the
version above, and prints each one. wasm-opt 132 accepts the same flags and
gives other bytes, so the version check matters.

## Build and compare

```
lygos/display-core/scripts/build-wasm.sh /some/empty/out-dir
```

It prints `built <sha256>`, `pinned <sha256>` and `MATCH` or `DIFFERENT
from the pin`. To build from several paths and require the pin:

```
lygos/display-core/scripts/check-reproducible.sh /some/empty/work-dir 3
```

## What must not vary, and what the script does about it

- **Environment.** Every tool runs under `env -i` with only `PATH`, `HOME`,
  `CARGO_HOME`, `RUSTUP_HOME` and the script's own `RUSTFLAGS`. So
  `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS` (which cargo prefers over
  `RUSTFLAGS`), `CC`, `CFLAGS` and the like in your shell cannot reach the
  build.
- **Cargo config files.** The script refuses if `$CARGO_HOME/config.toml`
  (or `config`) or any `.cargo/config.toml` (or `config`) above the crate
  exists, because those can set flags or profiles.
- **Build path.** Sources are read through the fixed path
  `/tmp/display-core-build/repo`, a symlink the script points at your
  checkout. The directory `/tmp/display-core-build` must be yours with mode
  700; the script creates it that way and refuses otherwise. Do not change
  this path: it is part of the output (cargo hashes path dependencies outside
  the workspace by absolute path). Only one build runs at a time (a lock in
  that directory).
- **Checkout path, cargo home, target dir.** These do not change the bytes
  (tested: several checkout paths, a separate cargo home, empty target dirs).
