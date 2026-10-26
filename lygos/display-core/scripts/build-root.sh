# Sourced by build-wasm.sh. Defines claim_build_root.
#
# The build reads its sources through one fixed path, BUILD_ROOT, because
# cargo hashes the absolute path of path dependencies outside the workspace
# and that hash changes the wasm bytes. BUILD_ROOT is a symlink inside
# BUILD_DIR.
#
# BUILD_DIR exists only while a build runs, and belongs to the user running
# it (mode 700, so nobody else can re-point the symlink):
#   - claim_build_root waits (up to DISPLAY_CORE_BUILD_WAIT seconds, default
#     900) while BUILD_DIR belongs to someone else, then creates its own;
#   - builds by the same user take turns through BUILD_DIR/lock;
#   - when the build exits, the symlink, the lock and BUILD_DIR are removed,
#     so the next user, or the next build, can claim it.
# No shared or world-writable directory and no administrator are needed. A
# BUILD_DIR left behind by a crashed build of another user blocks others
# (loudly, after the wait) until that user or an administrator removes it.

BUILD_DIR=/tmp/display-core-build
BUILD_ROOT=$BUILD_DIR/repo
BUILD_LOCK=$BUILD_DIR/lock

# Removes what this user created. The symlink's own owner is checked with
# stat (test -O would follow the link to its target).
release_build_root() {
  if [ -L "$BUILD_ROOT" ] && [ "$(stat -c %U "$BUILD_ROOT")" = "$(id -un)" ]; then
    unlink "$BUILD_ROOT"
  fi
  if [ -f "$BUILD_LOCK" ] && [ -O "$BUILD_LOCK" ]; then
    unlink "$BUILD_LOCK"
  fi
  if [ -d "$BUILD_DIR" ] && [ -O "$BUILD_DIR" ]; then
    rmdir "$BUILD_DIR" 2>/dev/null || true
  fi
}

claim_build_root() { # repo-dir
  local repo_dir=$1 wait=${DISPLAY_CORE_BUILD_WAIT:-900} deadline owner said=""
  deadline=$(($(date +%s) + wait))
  while :; do
    [ "$(date +%s)" -lt "$deadline" ] || fail "$BUILD_DIR still belongs to ${owner:-someone else} after $wait seconds"
    if [ ! -e "$BUILD_DIR" ] && ! mkdir -m 700 "$BUILD_DIR" 2>/dev/null; then
      [ -e "$BUILD_DIR" ] || fail "cannot create $BUILD_DIR"
      continue # someone created it first; look again
    fi
    [ -d "$BUILD_DIR" ] && [ ! -L "$BUILD_DIR" ] || fail "$BUILD_DIR is not a directory"
    owner=$(stat -c %U "$BUILD_DIR")
    if [ ! -O "$BUILD_DIR" ]; then
      [ -n "$said" ] || echo "waiting: $BUILD_DIR belongs to $owner (a build is running)"
      said=1
      sleep 5
      continue
    fi
    [ "$(stat -c %a "$BUILD_DIR")" = 700 ] || fail "$BUILD_DIR must have mode 700"
    exec 9>>"$BUILD_LOCK"
    flock -w "$wait" 9 || fail "another build of yours holds $BUILD_LOCK"
    # The previous holder may have removed the directory on its way out.
    if [ -d "$BUILD_DIR" ] && [ "$BUILD_LOCK" -ef "/proc/self/fd/9" ]; then
      break
    fi
    exec 9>&-
  done
  ln -sfn "$repo_dir" "$BUILD_ROOT"
  trap release_build_root EXIT
  [ "$(readlink "$BUILD_ROOT")" = "$repo_dir" ] || fail "$BUILD_ROOT does not point at $repo_dir"
}
