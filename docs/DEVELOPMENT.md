# Development Guide

## Toolchain

Rust 1.88 or newer (edition 2024). Git and Cargo are required for the git-build integration
tests; they are skipped with a message when absent.

## Everyday commands

```bash
cargo build                        # debug build
cargo run -- --home /tmp/ra list   # try the CLI against a throwaway home
cargo run -- --home /tmp/ra        # try the TUI against a throwaway home
cargo test --all-features          # unit + integration tests (offline)
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt
```

Use `--home` (or `RUSTARCADE_HOME`) during development so nothing is written to your real
configuration and data directories.

## Environment variables

| Variable | Effect |
|---|---|
| `RUSTARCADE_HOME` | Put config, data, cache and state under one directory |
| `RUSTARCADE_CATALOG_DIR` | Overlay a directory of manifests on the catalog (validated in local-source mode) |
| `RUSTARCADE_BUILTIN_CATALOG=0` | Disable the embedded catalog (tests) |
| `RUSTARCADE_OFFLINE=1` | Never touch the network |
| `RUSTARCADE_GITHUB_API`, `RUSTARCADE_CRATES_API` | API base URLs (mock servers in tests) |
| `RUSTARCADE_ALLOW_INSECURE_LOCAL=1` | Allow `http://127.0.0.1` and `file://` sources (tests only) |
| `RUSTARCADE_CARGO`, `RUSTARCADE_GIT` | Explicit tool paths |
| `RUSTARCADE_LOG` | tracing filter, e.g. `debug` or `rustarcade::install=trace` |
| `GITHUB_TOKEN` / `GH_TOKEN` | Optional token to raise the GitHub API rate limit |

## Test layout

- Unit tests live next to the code (`#[cfg(test)]`) and cover parsing, validation, path safety,
  asset selection, checksum parsing, archive extraction, version comparison, registry/library
  persistence, error formatting, the TUI reducer, and HTTP behaviour against `httpmock`.
- `tests/common/mod.rs` compiles `tests/fixtures/fixture_bin.rs` with `rustc` once per test
  binary. That program behaves as a fake `cargo install` (driven by crate name and an optional
  `fixture-version` file) and as a fake game (`--exit N`, `--sleep-ms N`, `--crash`, …).
- `tests/install_cargo_tests.rs` — install, play, update, rollback, cancel, concurrency,
  uninstall, crash recovery.
- `tests/install_release_tests.rs` — GitHub release flow against a mock API: checksum files,
  API digests, mismatches, raw binaries, updates, prereleases, missing assets.
- `tests/install_git_tests.rs` — real `git` + real `cargo build` on a local fixture repository.
- `tests/cli_tests.rs` — the binary end to end via `assert_cmd`.
- `tests/tui_state_tests.rs` — reducer flows and `TestBackend` rendering at several sizes.

Everything runs offline. Each test gets its own temporary home.

## Manual TUI check

`scripts/tui_smoke.py` (Linux/macOS) drives the real binary in a pseudo-terminal: it opens the
interface, searches, opens details, launches an installed game, sends Ctrl+C/Esc, and checks that
RustArcade comes back with the terminal restored. Run it after installing at least one game into
the home it is pointed at:

```bash
RUSTARCADE_HOME=/tmp/ra cargo run -- install snakeshell --yes
RUSTARCADE_HOME=/tmp/ra python3 scripts/tui_smoke.py target/debug/rustarcade
```

## Catalog maintenance

- `cargo run -- catalog validate` validates the embedded catalog; pass a path to validate a
  file or directory.
- `cargo run -- catalog index` regenerates `catalog/index.json`; a unit test fails when it is
  stale.
- Real-world verification: install each game into a throwaway home and launch it, then set
  `support_status = "verified"` and `verified_on`.

## Logging

Logs go to `state/logs/rustarcade.log.<date>` (never to the terminal while the TUI runs).
`--debug` raises the level to `debug` and, for CLI commands, mirrors log lines to stderr.
Per-install logs contain the exact command lines and tool output and are referenced by error
messages.

## Releasing

1. Update `CHANGELOG.md` and the version in `Cargo.toml`.
2. Tag `vX.Y.Z` and push; `release.yml` builds Linux (x86_64, aarch64), macOS (x86_64, aarch64)
   and Windows (x86_64) binaries with SHA-256 checksums and attaches them to a GitHub release.
