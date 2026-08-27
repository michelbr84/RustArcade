# Implementation Checklist

Status of the v0.1.0 scope. Every item below is implemented and covered by tests or the
verification steps listed in `DEVELOPMENT.md`.

## Foundation
- [x] Typed error hierarchy with actionable user messages and log paths
- [x] Platform-aware directories, `RUSTARCADE_HOME` override, atomic writes, corrupt-file quarantine
- [x] Platform/architecture detection, tool discovery, terminal info
- [x] Configuration file with defaults, validation and persistence
- [x] File logging (never to the TUI), per-install logs

## Catalog
- [x] Manifest schema with three typed installers and `deny_unknown_fields`
- [x] Strict validation (ids, URLs, paths, executables, digests, placeholders, OS subsets, duplicates)
- [x] Embedded built-in catalog (`build.rs`), local overlay, cached remote, remote sync with digests
- [x] `index.json` generation and staleness check
- [x] Search and ranking, category filters
- [x] 20 researched manifests, all verified by install + launch on Linux x86_64

## Installation
- [x] Installer selection with per-platform/tool fallbacks and skip reasons
- [x] Cargo installer (isolated root, version/features/bins, `.crates2.json` parsing)
- [x] GitHub release installer (resolve, pattern/heuristic asset selection, HTTPS download,
      SHA-256 from manifest/API digest/checksum file, safe extraction, binary discovery)
- [x] Git + Cargo build installer (shallow clone, reference/commit pin, workspace package, metadata version)
- [x] Transaction: staging → verify → swap → register → cleanup, rollback, startup sweep
- [x] Cancellation, progress phases, structured process execution without shells
- [x] Update checks (releases, crates.io, git remotes) with caching; update single/all
- [x] Uninstall limited to registered managed paths

## Launcher
- [x] Terminal session suspend/resume, resume guard, Ctrl+C isolation, stale-key drain
- [x] Exit code/signal capture, play history, recent list, statistics

## Interfaces
- [x] TUI: Home, Discover, Library, Favorites, Updates, Settings, Details, plan/progress/error/
      uninstall/quit/help/welcome/log dialogs, search, category filter, empty states, resize guard
- [x] CLI: list, search, info, install, play, update, uninstall, doctor, catalog update/validate/index, version, JSON output
- [x] Doctor with OK/WARNING/ERROR and repair guidance

## Quality
- [x] Unit tests, integration tests with local fixtures and mock servers, TUI reducer/render tests
- [x] `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, release build, doctor, catalog validate
- [x] CI (Linux/macOS/Windows), release workflow (5 targets), catalog validation workflow
- [x] README, SECURITY, CONTRIBUTING, CHANGELOG, product spec, architecture, threat model,
      development guide, catalog guide

## Deferred (post-v1)
- Signed remote catalog
- Optional sandboxing (bubblewrap / flatpak-spawn) on Linux
- Community catalogs with explicit trust prompts
- Additional themes and screenshots in the catalog
