#!/usr/bin/env bash
# Installs the cargo dev tools that the Nix flake would otherwise provide.
# Idempotent: skips anything already on PATH.
#
# Run from the repo root:  ./notes/install_tools.sh
set -euo pipefail

tools=(
  "cargo-nextest"
  "cargo-insta"
  "cargo-audit"
)

echo "==> Checking cargo dev tools"
to_install=()
for tool in "${tools[@]}"; do
  if command -v "$tool" >/dev/null 2>&1; then
    echo "    ✓ $tool already installed"
  else
    echo "    · $tool missing — will install"
    to_install+=("$tool")
  fi
done

if [ "${#to_install[@]}" -eq 0 ]; then
  echo "==> All tools present, nothing to do."
  exit 0
fi

echo "==> Installing: ${to_install[*]}"
# cargo-nextest refuses to build without --locked (its build is pinned to its
# own dep versions); --locked is harmless for the others too.
cargo install --locked "${to_install[@]}"

echo "==> Done. Versions:"
for tool in "${tools[@]}"; do
  if command -v "$tool" >/dev/null 2>&1; then
    printf '    %s: ' "$tool"
    "$tool" --version 2>/dev/null || echo "(installed)"
  fi
done
