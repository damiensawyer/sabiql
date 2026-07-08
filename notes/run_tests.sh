#!/usr/bin/env bash
# Run unit tests (nextest) and the psql integration tests (Tier 2).
# Starts the docker-compose Postgres if it isn't already up, and tears nothing
# down afterwards (so re-runs are fast).
#
# Port handling:
#   compose.yml maps the DB to host port 5433. If 5433 is already taken on the
#   host (e.g. another project's container), this script picks the next free
#   port, runs the compose service on it via an in-memory override file, and
#   exports SABIQL_TEST_DSN so the integration tests connect to the right port.
#   You can also force a port explicitly:  SABIQL_TEST_PORT=5544 ./notes/run_tests.sh
#
# Run from the repo root:  ./notes/run_tests.sh
set -euo pipefail

if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "✗ cargo-nextest not found. Run ./notes/install_tools.sh first." >&2
  exit 1
fi

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

# --- pick a host port for the compose DB -------------------------------------
DEFAULT_PORT=5433
HOST_PORT="${SABIQL_TEST_PORT:-$DEFAULT_PORT}"

port_in_use() { ss -ltn "sport = :$1" 2>/dev/null | grep -q ":$1"; }

if [ "$HOST_PORT" = "$DEFAULT_PORT" ] && port_in_use "$DEFAULT_PORT"; then
  # 5433 taken by something else — find a free alternative.
  for cand in 5434 5435 5436 5437 5438 5439 5440; do
    if ! port_in_use "$cand"; then HOST_PORT="$cand"; break; fi
  done
  if [ "$HOST_PORT" = "$DEFAULT_PORT" ]; then
    echo "✗ Host port $DEFAULT_PORT is in use and no free alternative found." >&2
    echo "  Stop the conflicting service, or set SABIQL_TEST_PORT=<port>." >&2
    exit 1
  fi
  echo "› Host port $DEFAULT_PORT busy — using $HOST_PORT instead."
fi

if [ "$HOST_PORT" != "$DEFAULT_PORT" ]; then
  export SABIQL_TEST_DSN="postgres://dev:dev@localhost:${HOST_PORT}/testdb"
fi

# --- compose override (only if remapping the port) ---------------------------
COMPOSE_FILES=(-f compose.yml)
OVERRIDE_FILE=""
if [ "$HOST_PORT" != "$DEFAULT_PORT" ]; then
  OVERRIDE_FILE="$(mktemp --suffix=.yml)"
  trap 'rm -f "$OVERRIDE_FILE"' EXIT
  cat > "$OVERRIDE_FILE" <<EOF
services:
  postgres:
    ports:
      - "${HOST_PORT}:5432"
EOF
  COMPOSE_FILES+=(-f "$OVERRIDE_FILE")
fi

echo "==> Ensuring Postgres is up (host port $HOST_PORT)"
docker compose "${COMPOSE_FILES[@]}" up -d --wait

if [ -n "${SABIQL_TEST_DSN:-}" ]; then
  echo "   SABIQL_TEST_DSN=$SABIQL_TEST_DSN"
fi

echo "==> Unit tests: cargo nextest run --workspace --all-features"
cargo nextest run --workspace --all-features

echo "==> Integration tests (Tier 2): tests::adapter_postgres"
cargo nextest run -p sabiql --run-ignored ignored-only \
  -E 'test(tests::adapter_postgres)'

echo "==> Reject unreferenced insta snapshots"
cargo insta test --unreferenced=reject -p sabiql

echo "==> Tests OK"
