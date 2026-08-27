<div align="center">

# ▲ RustArcade

**Your terminal. Your arcade.**

A Rust-powered terminal game launcher. Discover, install, update, and play open-source TUI and CLI
games directly from one terminal interface.

</div>

```text
 ▲ RUSTARCADE  Your terminal. Your arcade.        20 games · 3 installed · 1 update · 2h 14m played
 1 Home │ 2 Discover │ 3 Library │ 4 Favorites │ 5 Updates (1) │ 6 Settings
────────────────────────────────────────────────────────────────────────────────────────────────
╭ Discover (20) ───────────────────────────────────────╮╭ Preview ────────────────────────────╮
│ / chess                                    (Esc clear)││ Chess TUI                            │
│    Name             Category    Support   State       ││ Play chess in any terminal: local    │
│ ▶  Chess TUI        Board       verified  installed   ││ games, UCI engines, Lichess and      │
│    Tetro TUI        Puzzle      verified  update      ││ online play.                         │
│    TermFarm         Idle        verified  available   ││                                      │
│  ★ Terminal Poker   Card        verified  available   ││ Repository github.com/thomas-mauran… │
│    termitype        Typing      verified  available   ││ License    MIT                       │
│    Hammurabi        Strategy    verified  available   ││ Platforms  Linux, macOS, Windows     │
│    Rebels in the Sky Sports     verified  available   ││ Install    GitHub Release → Cargo    │
│    Sudoku TUI       Puzzle      verified  available   ││ Status     installed                 │
╰──────────────────────────────────────────────────────╯╰─────────────────────────────────────╯
 catalog: 20 remote games (fetched 3h ago)
────────────────────────────────────────────────────────────────────────────────────────────────
 ↑↓ navigate  Enter details  / search  c category  i install  p play  f favorite  x uninstall  q quit
```

RustArcade does **not** bundle games. It keeps a curated, declarative catalog of terminal games
and installs each one from its original project — a GitHub release, a crates.io package, or a
git checkout built with Cargo — into a directory it manages. Launching a game suspends the
interface, hands the terminal to the game, and brings you back when it exits.

## Features

- **Catalog** of real, verified terminal games (20 at launch) with search, category filters,
  support status, license, and platform information.
- **Three typed installers** — `github-release` (prebuilt binaries), `cargo` (`cargo install`
  into an isolated root), `git-cargo-build` (clone + `cargo build --release`). No shell snippets,
  ever.
- **Install & Play** in one keystroke: plan → checks → download/build → verify → register → run.
- **Transactional installs and updates**: staged, verified, atomically swapped, rolled back on
  failure; the previous version is kept until the new one is confirmed.
- **Integrity**: HTTPS only, SHA-256 verification from manifests, GitHub's per-asset digests, or
  release checksum files; archive extraction hardened against traversal and symlink attacks.
- **Library management**: installed games, updates (per game or all), uninstall that only removes
  what RustArcade created, favorites, recently played, play history and total play time.
- **Clean launcher lifecycle**: raw mode and the alternate screen are released before a game
  starts and restored afterwards — Ctrl+C, crashes and non-zero exits included.
- **Full CLI** sharing the same core as the TUI, `rustarcade doctor` diagnostics, JSON output.
- **Offline-first**: a built-in catalog ships in the binary; the remote catalog is cached and
  refreshed opportunistically. No accounts, no telemetry, no administrator privileges.

## Installation

RustArcade is a single binary. Requirements: a terminal of at least 60×16 characters; Cargo
and Git are optional and only needed for games that build from source.

**From source (Rust 1.88+):**

```bash
cargo install --git https://github.com/michelbr84/RustArcade rustarcade
```

or clone the repository and run `cargo build --release`; the binary is at
`target/release/rustarcade`.

**Prebuilt binaries** for Linux (x86_64, aarch64), macOS (Intel, Apple Silicon) and Windows
(x86_64) are attached to each GitHub release together with SHA-256 checksums.

## Usage

```bash
rustarcade                 # open the interface
rustarcade list            # catalog table
rustarcade search chess    # search by name, description or tag
rustarcade info chess-tui  # details, installers, install state, play stats
rustarcade install chess-tui --yes --play
rustarcade play chess-tui
rustarcade update --check  # what can be updated
rustarcade update --all    # update everything
rustarcade uninstall chess-tui
rustarcade doctor          # environment diagnostics
rustarcade catalog update  # refresh the remote catalog
rustarcade catalog validate [path]
rustarcade version
```

Global flags: `--home <dir>` (relocate all data, same as `RUSTARCADE_HOME`), `--offline`,
`--debug`. `list`, `search`, `info` and `doctor` accept `--json`.

### TUI controls

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` / `1`–`6` | Switch between Home, Discover, Library, Favorites, Updates, Settings |
| `↑` `↓` / `j` `k`, `PgUp` `PgDn`, `g` `G` | Move the selection |
| `Enter` | Open details · on details: **Play**, or **Install & Play** |
| `Esc` | Back / clear search |
| `/` | Search (Discover, Library, Favorites) |
| `c` | Cycle the category filter |
| `i` | Install |
| `p` | Play |
| `u` / `a` | Update selected / update all (Updates screen) |
| `x` | Uninstall (asks for confirmation) |
| `f` | Toggle favorite |
| `l` | View the last installation log |
| `r` | Refresh catalog / re-check updates |
| `?` | Keyboard help |
| `q` | Quit |

Before any installation RustArcade shows the source, method, version, destination, required
tools, checksum policy and a third-party notice. Installs run in the background with a live
progress dialog (Resolving → Downloading → Verifying → Extracting → Compiling → Registering →
Ready); errors are actionable and link to a log file.

## How games are installed

Each game has a TOML manifest in `catalog/games/`. A manifest declares *what* the game is and
*which typed installer* can fetch it — never a command line:

```toml
schema_version = 1
id = "chess-tui"
name = "Chess TUI"
summary = "Play chess in any terminal."
repository = "https://github.com/thomas-mauran/chess-tui"
license = "MIT"
categories = ["board", "strategy"]
support_status = "verified"

[compatibility]
os = ["linux", "macos", "windows"]

[run]
executable = "chess-tui"

[[installers]]
type = "github-release"
repository = "thomas-mauran/chess-tui"
asset = "chess-tui-{version}-{target}.tar.gz"

[[installers]]
type = "cargo"
crate = "chess-tui"
```

Installers are tried in manifest order; the first one whose tools are available and whose
source has something for your platform is used (`--method` forces one). Installations live in
`~/.local/share/rustarcade/games/<id>/current/` (platform equivalents on macOS and Windows) with
a launcher symlink in `…/bin/`. Cargo installs use `cargo install --root <that directory>` and
never touch your global `~/.cargo/bin`.

| Method | What happens | Tools needed |
|---|---|---|
| `github-release` | Resolve the latest (or pinned) release, pick the asset for your OS/CPU, download over HTTPS, verify SHA-256, extract safely, locate the binary | none |
| `cargo` | `cargo install --root <managed dir> <crate>` with optional version, features and `--bin` selection | Cargo |
| `git-cargo-build` | Shallow clone (optionally a tag/branch, optionally pinned to a commit), `cargo build --release`, copy the binary | Git, Cargo |

## Catalog

The catalog is the heart of RustArcade. Every entry is researched before it is added and marked
with a support status:

| Status | Meaning |
|---|---|
| **Verified** | Installed and launched successfully by RustArcade's verification run (date in `verified_on`) |
| Community Tested | Install path is unambiguous and reported working, not exercised by the maintainers |
| Experimental | Builds in principle; inactive or unproven upstream |
| Broken / Archived | Known not to work / upstream archived — never installed |

Launch catalog (all verified on Linux x86_64 on 2026-08-27): Chess TUI, Tetro TUI, TermFarm,
Terminal Poker, termitype, Hammurabi, Rebels in the Sky, Sudoku TUI, Setrixtui, BalatroTUI,
SnakeShell, Game of Life (gof-rs), TMaze, Rusty Lights, Mastermind, Retris, Snakers, 2048 TUI,
Go TUI, Connect Four.

Three catalog sources are merged at startup: the **built-in** catalog compiled into the binary,
the **cached** remote catalog, and the **remote** official catalog (`catalog/` in this
repository, fetched over HTTPS, integrity-checked file by file and validated before it replaces
the cache). RustArcade works fully offline and never fails to start because of the network.

Adding a game is a pull request with one TOML file — see `docs/ADDING-A-GAME.md`. CI validates
every manifest.

## Security model

- Manifests cannot express commands; unknown fields are rejected.
- Every external tool is executed with an explicit argument vector — no `sh -c`.
- Downloads are HTTPS-only with an HTTPS-only redirect policy and size caps.
- SHA-256 is verified whenever upstream publishes a digest (GitHub does so for every release
  asset); a mismatch aborts the install, deletes the artifact and exits with code 3.
- Archives are extracted with traversal, symlink, size and entry-count protections.
- RustArcade only deletes paths it recorded in its registry, all inside its own data directory.
- No administrator privileges, package managers, or upstream install scripts are ever used.

**Games remain third-party software.** RustArcade does not audit, sign, or sandbox them; a
launched game runs with your privileges. Details in [`SECURITY.md`](SECURITY.md) and
[`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md).

## Development

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
cargo run -- doctor
cargo run -- catalog validate
```

The test suite is offline: installers are exercised against a fixture program compiled with
`rustc` at test time, a mock GitHub/crates.io server, and a local git repository. See
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) and [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Contributing

Bug reports, catalog additions and features are welcome — read
[`CONTRIBUTING.md`](CONTRIBUTING.md). Catalog PRs are validated automatically.

## License

RustArcade is released under the [MIT License](LICENSE). Each game in the catalog is distributed
under its own license by its own authors; the catalog records the license where known.
# RustArcade
