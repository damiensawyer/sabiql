#!/usr/bin/env bash
# Build the whole workspace and run clippy with the exact flags CI uses.
# Run from the repo root:  ./notes/run_build.sh
set -euo pipefail

echo "==> cargo build --workspace"
cargo build --workspace

echo "==> cargo clippy (all features, -D warnings)"
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> Build + lint OK"
