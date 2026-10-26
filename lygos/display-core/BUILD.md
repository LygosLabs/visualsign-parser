# Building and verifying display-core

`WASM-SHA256` pins the sha256 of `display_core_bg.wasm` built from this
commit. Anyone with the tools below can rebuild it and must get exactly that
hash. `scripts/build-wasm.sh` exits 1 when the hash differs. Never re-pin a
different hash to make a check pass: a mismatch means the tools, the
environment or the source differ, and that must be explained.

## Where it builds: Ubuntu 24.04, x86_64, only

The pinned build has been reproduced only on Ubuntu 24.04 (noble) x86_64,
from several checkout paths and by a new user with an empty home. It is the
only supported verifier platform. Elsewhere:

- **Another Linux distribution:** untested. The script requires Ubuntu's
  clang build by its version line and the exact Ubuntu `libLLVM` and
  `libclang-cpp` files by sha256 (below). The Ubuntu .debs can be unpacked
  without root (route below), but they need a glibc at least as new as
  Ubuntu 24.04's. Bytes from such a host are unproven.
- **macOS:** cannot run the script. Apple and Homebrew clang are refused,
  and the script needs GNU `stat -c`, `sha256sum`, `ldd`, `nproc` and
  `flock`. Use an Ubuntu 24.04 x86_64 VM or container.
- **aarch64 (or any other host CPU):** unsupported. The packages and the
  binaryen tarball named here are x86_64, and whether another host triple
  gives the same bytes is unknown.

## What you need

| Tool | Exact version (first line of `--version`) or file sha256 | Where to get it |
|---|---|---|
| Rust | `rustc 1.94.0 (4a4ef493e 2026-03-02)` | rustup (https://rustup.rs). `rust-toolchain.toml` here selects 1.94.0 and the `wasm32-unknown-unknown` target; rustup installs both on first use. |
| clang | `Ubuntu clang version 18.1.3 (1ubuntu1)` | Ubuntu 24.04 package `clang-18` `1:18.1.3-1ubuntu1` |
| llvm-ar | `Ubuntu LLVM version 18.1.3` | package `llvm-18` `1:18.1.3-1ubuntu1` |
| libLLVM | `libLLVM.so.18.1` resolving to a file with sha256 `5c3a63957e28c15cdf1503892074f032d29b72e36a9824a77dfa87e48a0dafb0` | package `libllvm18` `1:18.1.3-1ubuntu1` (file `usr/lib/llvm-18/lib/libLLVM.so.1`) |
| libclang-cpp | `libclang-cpp.so.18.1` with sha256 `b9592746f7f8ff1156680fb1fad5faf5486df7a67aca8327cc959463416c6718` | package `libclang-cpp18` `1:18.1.3-1ubuntu1` (file `usr/lib/llvm-18/lib/libclang-cpp.so.18.1`) |
| wasm-bindgen | `wasm-bindgen 0.2.106` | `cargo install wasm-bindgen-cli --version 0.2.106 --locked` |
| wasm-opt | `wasm-opt version 133 (version_133)` | binaryen release `version_133`, `binaryen-version_133-x86_64-linux.tar.gz` from https://github.com/WebAssembly/binaryen/releases/tag/version_133, sha256 `2dc9c7813f5375db93d96ead4b78222fcc3e2677bbb832297af4797782a37489`. Put its `bin/` on `PATH`. |
| flock, ldd, git, sha256sum, stat | any | util-linux, libc-bin, git, coreutils |
| Node.js | 22 (only for the golden replay) | https://nodejs.org |

`scripts/build-wasm.sh` prints each tool's version and each library's path
and sha256, and refuses anything else. wasm-opt 132 accepts the same flags
and gives other bytes, so the checks matter.

The package files (sha256 of the `.deb`):

```
628b16701014ef7ad648380b20ea74b90dd543857f933b3e34d2fc042783de25  clang-18_1%3a18.1.3-1ubuntu1_amd64.deb
139cb82e16e75fcdd4a56562804ff9bfb482b65d0929580d621d28088075a27e  llvm-18_1%3a18.1.3-1ubuntu1_amd64.deb
d41631bb752815a8196261d1030c096591c59678c8adfbf2e6a740a691b49e80  libclang-common-18-dev_1%3a18.1.3-1ubuntu1_amd64.deb
af6197096ce3b89aab86de59f01c1b5feb008b2ae1514a03906c0664edb8b0b1  libllvm18_1%3a18.1.3-1ubuntu1_amd64.deb
e91eda104813ee0468d145d010a4a4e142401f3f17f9a65596c66d416298d018  libclang-cpp18_1%3a18.1.3-1ubuntu1_amd64.deb
```

### clang with root

```
sudo apt-get install clang-18=1:18.1.3-1ubuntu1 llvm-18=1:18.1.3-1ubuntu1 \
  libllvm18=1:18.1.3-1ubuntu1 libclang-cpp18=1:18.1.3-1ubuntu1
export PATH=/usr/lib/llvm-18/bin:$PATH
```

### clang without root

Download the five packages, check them against the sha256 list above, and
unpack them into a directory you own. Ubuntu's clang finds its libraries
through `RUNPATH $ORIGIN/../lib`, so the unpacked clang loads the unpacked
`libLLVM` and `libclang-cpp` without `LD_LIBRARY_PATH`:

```
mkdir -p ~/llvm-debs ~/llvm && cd ~/llvm-debs
apt-get download clang-18=1:18.1.3-1ubuntu1 llvm-18=1:18.1.3-1ubuntu1 \
  libclang-common-18-dev=1:18.1.3-1ubuntu1 libllvm18=1:18.1.3-1ubuntu1 \
  libclang-cpp18=1:18.1.3-1ubuntu1
sha256sum *.deb            # compare with the list above
for d in *.deb; do dpkg -x "$d" ~/llvm; done
export PATH=~/llvm/usr/lib/llvm-18/bin:$PATH
ldd ~/llvm/usr/lib/llvm-18/bin/clang | grep -E 'LLVM|clang-cpp'   # both under ~/llvm
```

The build also needs network access the first time (crates.io, and
github.com for the `tkhq/qos` git dependency of the upstream workspace).

## Build and compare

```
lygos/display-core/scripts/build-wasm.sh /some/empty/out-dir
```

It prints `built <sha256>`, `pinned <sha256>` and `MATCH` (exit 0), or
`DIFFERENT from the pin` (exit 1). To build from several paths and require
the pin:

```
lygos/display-core/scripts/check-reproducible.sh /some/empty/work-dir 3
```

Only after a reviewed source change, to produce the new pin:
`build-wasm.sh --allow-different <out-dir>` (prints the same, exits 0).

## What must not vary, and what the script does about it

- **Environment.** Every tool runs under `env -i` with only `PATH`, `HOME`,
  `CARGO_HOME`, `RUSTUP_HOME` and the script's own `RUSTFLAGS`. So
  `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS` (which cargo prefers over
  `RUSTFLAGS`), `CC`, `CFLAGS`, `LD_LIBRARY_PATH` and the like in your shell
  cannot reach the build.
- **Cargo config files.** The script refuses if `$CARGO_HOME/config.toml`
  (or `config`) or any `.cargo/config.toml` (or `config`) above the crate
  exists, because those can set flags or profiles.
- **Build path.** Sources are read through the fixed path
  `/tmp/display-core-build/repo`, a symlink the script points at your
  checkout. Do not change this path: it is part of the output (cargo hashes
  path dependencies outside the workspace by absolute path).
  `/tmp/display-core-build` exists only while a build runs: the script
  creates it as yours with mode 700 (so nobody else can re-point the
  symlink) and removes it when the build exits. On a machine several users
  share, a second user's build waits (up to 15 minutes) while another user's
  build holds it, then takes its turn; no administrator or world-writable
  directory is needed. Builds by the same user take turns through a lock in
  that directory. A directory left behind by another user's crashed build
  makes the wait time out with an error naming its owner; that user (or an
  administrator) removes it.
- **Checkout path, cargo home, target dir, job count.** These do not change
  the bytes (tested).
