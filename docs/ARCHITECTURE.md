# Architecture

RustArcade is a single crate with a library (`src/lib.rs`) and a thin binary (`src/main.rs`).
Both user interfaces call the same application core.

```
            ┌──────────────┐        ┌──────────────┐
            │  CLI (clap)  │        │ TUI (ratatui)│
            └──────┬───────┘        └──────┬───────┘
                   └──────────┬────────────┘
                        ┌─────▼──────┐
                        │  Services  │  src/services.rs — the one facade
                        └─────┬──────┘
   ┌──────────┬───────────────┼───────────────┬──────────────┐
┌──▼───┐ ┌────▼────┐ ┌────────▼───────┐ ┌─────▼─────┐ ┌──────▼──────┐
│Catalog│ │Registry │ │   Installers   │ │ Launcher  │ │   Library   │
│       │ │& Config │ │ cargo/release/ │ │ terminal  │ │ favorites,  │
│       │ │         │ │ git + txn      │ │ lifecycle │ │ history     │
└──────┘ └─────────┘ └───────┬────────┘ └───────────┘ └─────────────┘
                     ┌───────▼────────┐
                     │  net: HTTPS,   │
                     │ GitHub, crates │
                     └────────────────┘
```

## Module map

| Module | Responsibility |
|---|---|
| `error` | Typed error enums per domain, `Error` umbrella, `UserMessage` (title/detail/causes/hint/log) |
| `paths` | `AppPaths` (platform dirs or `RUSTARCADE_HOME`), atomic writes, path safety (`safe_relative`, `is_within`) |
| `platform` | OS/arch detection, target triples, tool discovery (cargo/git/rustc), terminal info |
| `version` | Lenient semver parsing and comparison |
| `config` | `config.toml` model with defaults and validation |
| `logging` | tracing to daily log files; per-install `InstallLog` |
| `catalog::manifest` | Manifest types (`GameManifest`, `InstallerSpec` tagged enum, `GameId`) |
| `catalog::validate` | Every validation rule; cross-file checks; parse-error mapping with line/column |
| `catalog` | Loading (embedded via `build.rs`, directories), merging, `Catalog` |
| `catalog::index` / `catalog::remote` | `index.json` generation and remote catalog sync with digest checks and atomic swap |
| `catalog::search` | Ranking |
| `registry` | Installed-game records (version, installer, executable, managed paths) |
| `library` | Favorites, recent list, play sessions and statistics |
| `net` | HTTPS-only client (redirect policy, streaming SHA-256 downloads, size caps), GitHub releases client (ETag cache, rate-limit handling, asset digests), crates.io client |
| `install` | `Phase`/`ProgressEvent`, `InstallPlan`, installer selection, `check_latest` |
| `install::cargo` / `github_release` / `git_build` | The three typed installers |
| `install::assets` | Release asset selection (patterns + heuristics) and checksum-file parsing |
| `install::archive` | Safe extraction, binary discovery, executable verification |
| `install::process` | Structured process execution with logging and cancellation |
| `install::transaction` | Staging → verify → swap → register → cleanup; uninstall; startup sweep |
| `launcher` | `TerminalSession` trait, `InterruptFlag`, `launch()` with a resume guard |
| `services` | `Services`: state derivation (`GameState`), views, install/update/uninstall/play orchestration, catalog refresh |
| `doctor` | Diagnostics |
| `cli` | clap definitions and output formatting |
| `tui` | `TuiApp` reducer (`update(Message) -> Vec<Effect>`), keymap, screens, modals, event loop |

## Catalog model

A manifest is data, not code. `InstallerSpec` is an internally tagged enum, so the `type` field
selects exactly one of `CargoSpec`, `GithubReleaseSpec`, `GitCargoBuildSpec`; all structs use
`deny_unknown_fields`. Validation runs at parse time (ids), per manifest (every field), and
across the set (duplicate ids, duplicate executables, file-name/id match). The built-in catalog
is validated by a unit test, so an invalid manifest cannot ship.

At runtime the `Catalog` is a merge of built-in → cached remote → optional local directory
(`RUSTARCADE_CATALOG_DIR`, used by tests), keyed by id.

## Installer architecture

`install::plan` walks the manifest's installers in order (optionally forced with `--method`),
skips those whose tools are missing or whose sources have nothing for this platform, and
returns an `InstallPlan` carrying the resolved release/asset/checksum (so the confirmation shows
exactly what will happen). `install::fetch` dispatches to the chosen installer, which produces
files in a staging directory and returns a `Fetched` descriptor (executable path, version,
`InstallSource`). The transaction then verifies and commits.

All three installers produce the same layout, `current/bin/<executable>`, which keeps the
launcher, registry and uninstall logic uniform.

## Launcher lifecycle

```
Services::play → launch_spec (registry + manifest args/env/cwd, containment check)
  → session.suspend()   (TUI: raw mode off, leave alt screen, show cursor)
  → interrupt.enter_child_mode()   (Ctrl+C now only reaches the game)
  → Command::spawn with inherited stdio → wait
  → guard.finish(): interrupt.leave_child_mode(); session.resume()
        (TUI: enter alt screen, raw mode on, drain stale keys, recreate the Terminal so the
         next frame repaints everything without querying the terminal)
  → PlaySession recorded → TUI shows a toast on the same screen
```

The guard is a `Drop` type, so resume also runs when spawning fails or a panic unwinds.

## Storage design

```
config/config.toml                     user settings
data/games/<id>/current/bin/<exe>      installed game (staging-*/previous-* during swaps)
data/bin/<exe>                         launcher symlink (copy on Windows)
data/catalog/{index.json,games/*.toml} cached remote catalog
data/state/registry.json               installations and managed paths
data/state/library.json                favorites + recent
data/state/history.json                play sessions
cache/downloads/<id>/                  in-flight downloads
cache/build/<id>-<nonce>/              git checkouts + target dirs (removed after use)
cache/github/*.json                    ETag cache for GitHub API responses
cache/update-check.json                cached update results
state/logs/rustarcade.log.<date>       application log
state/logs/install-<id>-<ts>.log       per-installation logs
```

All state files are written atomically (temp file + rename) and corrupt files are quarantined
rather than crashing startup. `RUSTARCADE_HOME` relocates everything under one root.

## Concurrency model

The main thread owns the terminal and is the only reader of stdin. A tokio multi-thread
runtime executes planning, installs, update checks and catalog refreshes; results flow back
through an unbounded channel the TUI drains every tick (50 ms). Game launches run synchronously
on the main thread. Per-game guards (`active_jobs`, `running`) prevent concurrent installs,
updates, uninstalls or launches of the same game.
