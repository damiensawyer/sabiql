#!/usr/bin/env bash
# Run unit tests (nextest) and the psql integration tests (Tier 2).
# Starts the docker-compose Postgres if it isn't already up, and tears nothing
# down afterwards (so re-runs are fast).
#
# Run from the repo root:  ./notes/run_tests.sh
set -euo pipefail

if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "✗ cargo-nextest not found. Run ./notes/install_tools.sh first." >&2
  exit 1
fi

echo "==> Ensuring Postgres is up (docker compose)"
docker compose up -d --wait

echo "==> Unit tests: cargo nextest run --workspace --all-features"
cargo nextest run --workspace --all-features

echo "==> Integration tests (Tier 2): tests::adapter_postgres"
cargo nextest run -p sabiql --run-ignored ignored-only \
  -E 'test(tests::adapter_postgres)'

echo "==> Reject unreferenced insta snapshots"
cargo insta test --unreferenced=reject -p sabiql

echo "==> Tests OK"
