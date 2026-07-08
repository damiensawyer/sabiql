# sabiql — local dev setup notes

## What is `nix develop`?

[Nix](https://nixos.org/) is a **reproducible, declarative package manager**. The `flake.nix`
in this repo declares a `devShell` — a fully isolated development environment containing exact
versions of: the Rust toolchain (1.96.0 with clippy / rust-analyzer / rustfmt), `cargo-nextest`,
`cargo-insta`, `cargo-audit`, `graphviz`, `postgresql`, `lefthook`, etc.

- `nix develop` "enters" that shell — temporarily putting all those exact-version tools on your
  `PATH` for the current terminal only. Nothing is installed globally; nothing pollutes your host.
  When you exit the shell, they're gone.
- `direnv` + `.envrc` (which just says `use flake .`) automates this: every time you `cd` into the
  repo, the Nix shell loads automatically. No manual `nix develop` needed.

The point: **bit-for-bit reproducible dev envs**, so every contributor has identical tools
regardless of host OS. It's the "one command and everything just works" path — *if* you have
Nix installed.

---

## Two paths to a local env

### Path A — Use Nix (the maintainer's recommended way)

Install Nix (with flakes) and direnv. On Arch:

```bash
# Determinate installer — flakes enabled out of the box (recommended):
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install

# Or the official Nix installer, then enable flakes in /etc/nix/nix.conf:
#   experimental-features = nix-command flakes
# sh <(curl -L https://nixos.org/nix/install) --daemon

sudo pacman -S direnv          # then add the direnv hook to your shell rc
cd ~/code/sabisql/main
direnv allow                   # loads the flake shell automatically from now on

cargo nextest --version        # should now be available
```

### Path B — Native Arch (less new machinery, what I use here)

The host already has `rustc 1.96.0` (matches `rust-toolchain.toml` exactly), `cargo`, `psql`,
and `docker`. Only the **cargo tools the flake provides** are missing locally — install them
with `notes/install_tools.sh`:

```bash
./notes/install_tools.sh        # installs cargo-nextest, cargo-insta, cargo-audit
# optionally:
sudo pacman -S graphviz lefthook

# verify
cargo build --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace
```

That mirrors exactly what `.github/workflows/ci.yml` runs in CI, so local results match the
maintainers' checks.

---

## Repo-specific gotchas

- Cargo **workspace** with 4 crates: `src/{domain,app,infra,ui}` plus the root binary crate.
- `rustfmt.toml` + `clippy.toml` + `scripts/lint_all.sh` (test-name linter) are enforced in CI —
  run `cargo fmt` before committing (lefthook auto-fixes format on pre-commit if installed).
- Snapshots via `cargo-insta` are committed; CI rejects unreferenced snapshots
  (`cargo insta test --unreferenced=reject`).

## Integration tests (the psql Tier 2 tests)

These need a live Postgres. The repo ships a docker-compose setup:

```bash
docker compose up -d --wait
#   dev/dev @ localhost:5433, database testdb (pre-seeded via scripts/seed.sql)
cargo nextest run -p sabiql --run-ignored ignored-only -E 'test(tests::adapter_postgres)'
```

## Convenience scripts

All in `notes/`:

| script              | what it does                                                        |
| ------------------- | ------------------------------------------------------------------- |
| `install_tools.sh`  | installs the cargo dev tools the Nix flake would otherwise provide  |
| `run_build.sh`      | `cargo build --workspace` + clippy with the same flags as CI        |
| `run_tests.sh`      | starts the compose DB, then runs unit + integration tests           |
| `wipe_builds.sh`    | effective `clean` — removes `target/` and nextest profile artifacts |

Run them from the repo root, e.g. `./notes/run_build.sh`.
