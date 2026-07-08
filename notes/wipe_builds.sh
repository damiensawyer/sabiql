#!/usr/bin/env bash
# Effective `clean`: removes Cargo build output and nextest profile artifacts.
# Does NOT touch the git tree or the docker compose volume.
#
# Run from the repo root:  ./notes/wipe_builds.sh
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

removed=0

if [ -d target ]; then
  echo "==> Removing target/ ($(du -sh target 2>/dev/null | cut -f1))"
  rm -rf target
  removed=1
fi

# nextest writes JUnit/profile dirs under target/, so the rm above covers them,
# but clean up any stray profile dirs just in case nextest used a custom path.
shopt -s nullglob
for d in target.nextest nextest-archive; do
  if [ -d "$d" ]; then
    echo "==> Removing $d/"
    rm -rf "$d"
    removed=1
  fi
done
shopt -u nullglob

# Clear Cargo's incremental cache (per-crate, under the registry).
registry_inc="${CARGO_HOME:-$HOME/.cargo}/registry/cache"
if [ -d "$registry_inc" ]; then
  echo "==> Leaving cargo registry cache intact ($registry_inc) — not a build artifact"
fi

if [ "$removed" -eq 0 ]; then
  echo "==> Nothing to wipe (no target/ build artifacts found)."
else
  echo "==> Build artifacts wiped."
fi
