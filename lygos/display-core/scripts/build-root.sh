# Sourced by build-wasm.sh. Defines claim_build_root.
#
# The build reads its sources through one fixed path, BUILD_ROOT, because
# cargo hashes the absolute path of path dependencies outside the workspace
# and that hash changes the wasm bytes. BUILD_ROOT is a symlink in BUILD_DIR.
#
# claim_build_root <repo-dir>
#   - BUILD_DIR must be a real directory that is either yours with mode 700
#     (or 1777), or root's with mode 1777 (sticky, like /tmp itself) so several
#     users can share it. On a shared machine an admin creates it once:
#       sudo install -d -m 1777 /tmp/display-core-build
#     A directory owned by another user is refused: its owner could re-point
#     your symlink.
#   - Takes the lock BUILD_DIR/lock on fd 9 for the rest of the build (opened
#     read-only if another user created it; flock works either way).
#   - Points BUILD_ROOT at <repo-dir> (refusing a symlink left by another
#     user) and removes it again when the build exits, so the next user, or
#     the next build, can claim it.

BUILD_DIR=/tmp/display-core-build
BUILD_ROOT=$BUILD_DIR/repo

claim_build_root() {
  local repo_dir=$1 me mode owner
  me=$(id -un)
  [ -e "$BUILD_DIR" ] || mkdir -m 700 "$BUILD_DIR"
  [ -d "$BUILD_DIR" ] && [ ! -L "$BUILD_DIR" ] || fail "$BUILD_DIR is not a directory"
  mode=$(stat -c %a "$BUILD_DIR")
  owner=$(stat -c %U "$BUILD_DIR")
  if [ "$owner" = "$me" ]; then
    [ "$mode" = 700 ] || [ "$mode" = 1777 ] ||
      fail "$BUILD_DIR must have mode 700 (or 1777), it has $mode"
  elif [ "$owner" = root ]; then
    [ "$mode" = 1777 ] || fail "$BUILD_DIR is root's; it must have mode 1777, it has $mode"
  else
    fail "$BUILD_DIR belongs to $owner; only you or root (mode 1777) may own it"
  fi

  local lock=$BUILD_DIR/lock
  if [ -e "$lock" ] && [ ! -O "$lock" ]; then
    exec 9<"$lock"
  else
    exec 9>"$lock"
    chmod 644 "$lock"
  fi
  flock -w 900 9 || fail "another display-core build holds $lock"

  if [ -L "$BUILD_ROOT" ] && [ ! -O "$BUILD_ROOT" ]; then
    fail "$BUILD_ROOT was left by $(stat -c %U "$BUILD_ROOT"); it must be removed first"
  fi
  ln -sfn "$repo_dir" "$BUILD_ROOT"
  trap 'if [ -L "$BUILD_ROOT" ] && [ -O "$BUILD_ROOT" ]; then rm -f "$BUILD_ROOT"; fi' EXIT
  [ "$(readlink "$BUILD_ROOT")" = "$repo_dir" ] || fail "$BUILD_ROOT does not point at $repo_dir"
}
