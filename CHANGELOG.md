# Changelog

All notable changes to RustArcade are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- README: install from crates.io (`cargo install rustarcade`) and status badges.

## [0.1.0] - 2026-08-27

Initial release.

### Added
- Ratatui interface with Home, Discover, Library, Favorites, Updates, Settings and Game Details
  screens; search, category filters, favorites, recently played, play history and total play time.
- Command-line interface sharing the same core: `list`, `search`, `info`, `install`, `play`,
  `update`, `uninstall`, `doctor`, `catalog update|validate|index`, `version`, with `--json`
  output where useful.
- Declarative TOML game catalog with strict validation and three typed installers:
  `github-release`, `cargo`, `git-cargo-build`.
- Built-in catalog of 20 verified terminal games, embedded in the binary, plus a remote catalog
  mechanism with per-file SHA-256 verification and atomic replacement.
- Transactional install/update pipeline (staging → verify → swap → register) with rollback,
  leftover recovery at startup, per-installation logs, and cancellation.
- Launcher that suspends the interface, runs the game interactively, records the session, and
  restores the terminal on every exit path.
- Update detection against GitHub releases, crates.io and git remotes.
- Security hardening: HTTPS-only networking, checksum verification (manifest, GitHub asset
  digests, release checksum files), safe archive extraction, managed-directory containment,
  no shell execution, no privilege escalation.
- `doctor` diagnostics with repair guidance.
- GitHub Actions workflows for CI (Linux/macOS/Windows), release binaries for five targets, and
  catalog validation on pull requests.
- Documentation: README, SECURITY, CONTRIBUTING, product specification, architecture, threat
  model, development guide, catalog contribution guide.
