# RustArcade — Product Specification

## Purpose

RustArcade is a terminal game launcher and catalog. It lets a user discover open-source
terminal (TUI/CLI) games, install them from their original projects with one action, keep them
updated, and play them without leaving the terminal. It never bundles or redistributes games.

## Core experience

```
Open RustArcade → Browse catalog → Select game → View details → Install & Play
  → RustArcade checks platform and tools → downloads or builds → verifies → registers
  → suspends its interface → runs the game interactively → game exits
  → RustArcade restores the terminal → returns to the previous screen
```

## Users

- People who enjoy terminal games and want a curated, safe way to find and run them.
- Developers of terminal games who want a low-friction distribution channel: one TOML file.

## Functional requirements

### Catalog
- Browse, search (name, description, tags) and filter (category, installed, favorites).
- Each entry shows name, summary, description, repository, license, categories, tags, platform
  support, installer methods, support status, installed and latest version, play statistics.
- Sources: built-in (compiled in), cached remote, remote official. Remote failures never block
  startup; remote data is validated before acceptance.

### Installation
- Typed installers only: `github-release`, `cargo`, `git-cargo-build`.
- A plan is shown before any change: source, method, version, destination, tools, checksum
  policy, warnings, "administrator: not required".
- Installs are transactional and logged; progress is visible and cancellable; failures produce
  actionable errors with a log path.
- States: Available, Installing, Installed, Update available, Running, Broken, Unsupported.

### Launching
- Games run as interactive children of RustArcade with the terminal fully handed over.
- Exit code, signal, start/end time and duration are recorded.
- The terminal is restored on normal exit, non-zero exit, crash, Ctrl+C and launch errors.

### Updates and removal
- Update checks compare installed versions with the latest release / crate / git commit.
- Updates preserve the previous installation until the new one is verified.
- Uninstall removes only RustArcade-owned files and asks for confirmation; save data outside
  RustArcade is never touched.

### Diagnostics
- `doctor` checks OS, architecture, terminal, directories, Git, Cargo, rustc, network, catalog
  validity and registry integrity, and reports OK / WARNING / ERROR with repair guidance.

### Interfaces
- TUI: Home, Discover, Library, Favorites, Updates, Settings, Game Details, install plan,
  progress and error dialogs; keyboard only; responsive to resizing; empty states.
- CLI: `list`, `search`, `info`, `install`, `play`, `update`, `uninstall`, `doctor`,
  `catalog update|validate|index`, `version`; the same services as the TUI.

## Non-functional requirements

- Security first (see `THREAT-MODEL.md`); no shell execution, no elevated privileges.
- Offline capable; fast startup (catalog embedded).
- Cross-platform: Linux, macOS, Windows; x86_64 and aarch64.
- Actionable errors, logs never written to the TUI.
- Tested without network access.

## Non-goals (v1)

User accounts, cloud saves, friends, ratings, achievements, web or mobile clients, storefronts,
multiplayer matchmaking, streaming, remote SSH hosting, sandboxing games.
