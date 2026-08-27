# RustArcade
## Complete Product, Architecture, Security, and Implementation Plan

> **RustArcade** is a terminal-based game launcher and catalog written in Rust.  
> It does **not** bundle games. Instead, it presents a curated TUI catalog of compatible terminal games and lets the user automatically **discover, download, install, update, launch, and manage** them from one interface.

---

## 1. Product Vision

RustArcade should feel like a lightweight **Steam-style launcher for terminal games**, while remaining:

- fast;
- local-first;
- open source;
- cross-platform where possible;
- safe by default;
- easy to extend;
- independent from the games it launches.

The core principle is simple:

```text
RustArcade
    ↓
Browse Catalog
    ↓
Select Game
    ↓
Check Compatibility
    ↓
Install if Needed
    ↓
Launch Game
    ↓
Return to RustArcade
```

RustArcade is responsible for the **launcher experience**, not for owning or embedding the games.

---

# 2. Core Goals

RustArcade should provide the following capabilities.

## 2.1 Game Discovery

Users should be able to:

- browse available terminal/TUI games;
- search by name;
- filter by category;
- filter by language;
- filter by installation method;
- see repository information;
- see whether a game is installed;
- see whether an update is available;
- mark favorites;
- view recently played games.

---

## 2.2 Automated Installation

When the user selects a game that is not installed, RustArcade should:

1. inspect the game manifest;
2. detect the current operating system and architecture;
3. detect required tools;
4. choose the safest supported installation method;
5. show the installation plan;
6. install the game;
7. verify the resulting executable;
8. save the installation metadata;
9. launch the game.

The preferred installation order should be:

```text
Prebuilt release binary
        ↓
Package manager
        ↓
cargo install
        ↓
Build from source
```

Building from source should normally be the last fallback.

---

# 3. Non-Goals

RustArcade should **not**:

- redistribute third-party game source code;
- copy game assets into the RustArcade repository;
- modify third-party projects unless explicitly required;
- execute arbitrary shell scripts from untrusted manifests;
- silently install system packages;
- require users to create an account;
- require a centralized RustArcade server for normal operation;
- replace Cargo, Git, GitHub, or package managers.

---

# 4. Recommended Technology Stack

## Language

```text
Rust
```

Rust is ideal for:

- terminal applications;
- fast startup;
- static binaries;
- cross-platform support;
- process management;
- safe concurrency;
- integration with Cargo;
- strong ecosystem support for TUIs.

---

## TUI Framework

Recommended:

```text
ratatui
```

Terminal backend:

```text
crossterm
```

Suggested crates:

```toml
ratatui
crossterm
serde
serde_json
toml
reqwest
tokio
anyhow
thiserror
directories
semver
sha2
url
chrono
clap
tracing
tracing-subscriber
```

Optional:

```toml
git2
zip
tar
flate2
xz2
indicatif
which
tempfile
uuid
open
```

---

# 5. High-Level Architecture

```text
┌────────────────────────────────────────────────────────────┐
│                       RustArcade                           │
├────────────────────────────────────────────────────────────┤
│                        TUI Layer                           │
│                                                            │
│  Home │ Library │ Catalog │ Favorites │ Updates │ Settings │
├────────────────────────────────────────────────────────────┤
│                    Application Core                        │
│                                                            │
│ Catalog Manager                                            │
│ Game Manager                                               │
│ Installer                                                  │
│ Launcher                                                   │
│ Update Manager                                             │
│ Dependency Detector                                        │
│ Platform Detector                                          │
│ State Manager                                              │
├────────────────────────────────────────────────────────────┤
│                       Providers                            │
│                                                            │
│ GitHub Releases │ Cargo │ Git │ Local Binary │ Packages    │
├────────────────────────────────────────────────────────────┤
│                     Local Storage                          │
│                                                            │
│ config.toml                                                │
│ catalog cache                                              │
│ installed games metadata                                   │
│ logs                                                       │
│ favorites                                                  │
│ play history                                               │
└────────────────────────────────────────────────────────────┘
```

---

# 6. Recommended Project Structure

```text
rustarcade/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
├── CHANGELOG.md
│
├── src/
│   ├── main.rs
│   │
│   ├── app/
│   │   ├── mod.rs
│   │   ├── state.rs
│   │   ├── events.rs
│   │   └── actions.rs
│   │
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── terminal.rs
│   │   ├── layout.rs
│   │   ├── theme.rs
│   │   ├── widgets/
│   │   └── screens/
│   │       ├── home.rs
│   │       ├── catalog.rs
│   │       ├── game_details.rs
│   │       ├── library.rs
│   │       ├── favorites.rs
│   │       ├── updates.rs
│   │       └── settings.rs
│   │
│   ├── catalog/
│   │   ├── mod.rs
│   │   ├── manifest.rs
│   │   ├── loader.rs
│   │   ├── validator.rs
│   │   └── cache.rs
│   │
│   ├── installer/
│   │   ├── mod.rs
│   │   ├── strategy.rs
│   │   ├── github_release.rs
│   │   ├── cargo_install.rs
│   │   ├── git_build.rs
│   │   └── archive.rs
│   │
│   ├── launcher/
│   │   ├── mod.rs
│   │   ├── process.rs
│   │   └── terminal.rs
│   │
│   ├── platform/
│   │   ├── mod.rs
│   │   ├── os.rs
│   │   ├── architecture.rs
│   │   └── dependencies.rs
│   │
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── config.rs
│   │   ├── library.rs
│   │   ├── history.rs
│   │   └── paths.rs
│   │
│   ├── github/
│   │   ├── mod.rs
│   │   └── api.rs
│   │
│   └── error.rs
│
├── catalog/
│   ├── games/
│   │   ├── chess-tui.toml
│   │   ├── tetro-tui.toml
│   │   ├── balatro-tui.toml
│   │   └── ...
│   └── schema/
│       └── game-manifest.schema.json
│
├── tests/
│   ├── catalog.rs
│   ├── installer.rs
│   ├── launcher.rs
│   └── fixtures/
│
└── .github/
    └── workflows/
        ├── ci.yml
        ├── release.yml
        └── catalog-validation.yml
```

---

# 7. Game Manifest System

The catalog is the most important architectural decision.

Each supported game should have a small declarative manifest.

RustArcade should **never need custom Rust code for every game**.

Example:

```toml
schema_version = 1

id = "chess-tui"
name = "Chess TUI"
repository = "https://github.com/thomas-mauran/chess-tui"
description = "Play chess in any terminal with Stockfish and Lichess support."

category = ["board", "strategy", "chess"]

license = "MIT"

[compatibility]
linux = true
macos = true
windows = true

[run]
command = "chess-tui"

[[installers]]
type = "cargo"
crate = "chess-tui"

[[installers]]
type = "github-release"
repository = "thomas-mauran/chess-tui"
```

---

# 8. Manifest Design

A full manifest can contain:

```toml
schema_version = 1

id = "example-game"
name = "Example Game"
description = "Example terminal game."

homepage = "https://github.com/example/game"
repository = "https://github.com/example/game"

authors = ["Example Developer"]
license = "MIT"

category = [
  "arcade",
  "strategy"
]

tags = [
  "rust",
  "terminal",
  "tui"
]

[compatibility]
linux = true
macos = true
windows = false

architectures = [
  "x86_64",
  "aarch64"
]

[run]
command = "example-game"
args = []

[requirements]
commands = ["git"]

[[installers]]
type = "github-release"
repository = "example/game"

[[installers]]
type = "cargo"
crate = "example-game"
```

---

# 9. Avoid Arbitrary Shell Commands

This is extremely important.

Do **not** design manifests like this:

```toml
install = "curl example.com/script.sh | bash"
```

or:

```toml
install = "git clone ... && cd ... && sudo make install"
```

That would make the catalog effectively a remote code execution system.

Instead, manifests should use structured operations.

Good:

```toml
[[installers]]
type = "cargo"
crate = "game-name"
```

Good:

```toml
[[installers]]
type = "github-release"
repository = "owner/repository"
asset_pattern = "game-{os}-{arch}"
```

Good:

```toml
[[installers]]
type = "git-cargo-build"
repository = "https://github.com/example/game"
binary = "game"
```

RustArcade itself decides which commands are allowed.

---

# 10. Installation Providers

RustArcade should support several installer providers.

## Provider A — GitHub Releases

Preferred whenever possible.

Workflow:

```text
GitHub Repository
       ↓
Latest Release
       ↓
Select OS/Architecture Asset
       ↓
Download
       ↓
Verify Checksum
       ↓
Extract
       ↓
Place Binary
       ↓
Register Installation
```

Advantages:

- fast;
- no compiler required;
- smaller attack surface;
- easy updates.

---

## Provider B — Cargo Install

Example:

```bash
cargo install chess-tui
```

RustArcade would execute Cargo using structured process arguments.

Conceptually:

```rust
Command::new("cargo")
    .args(["install", "chess-tui"])
```

Never:

```rust
Command::new("sh")
    .arg("-c")
    .arg("cargo install chess-tui");
```

This avoids unnecessary shell interpretation.

---

# 11. RustArcade-Managed Install Directory

Do not depend exclusively on the global Cargo bin directory.

Recommended:

```text
~/.local/share/rustarcade/
```

Linux example:

```text
~/.local/share/rustarcade/
├── games/
│   ├── chess-tui/
│   ├── tetro-tui/
│   └── sudoku-tui/
│
├── bin/
│   ├── chess-tui
│   ├── tetro-tui
│   └── sudoku-tui
│
├── cache/
├── catalog/
├── logs/
└── state/
```

Use the Rust `directories` crate to choose the correct platform-specific directory.

---

# 12. Installation State

RustArcade should maintain metadata such as:

```json
{
  "id": "chess-tui",
  "version": "0.8.2",
  "installed_at": "2026-08-27T12:00:00Z",
  "installer": "github-release",
  "executable": "/home/user/.local/share/rustarcade/bin/chess-tui",
  "repository": "thomas-mauran/chess-tui"
}
```

This makes uninstalling and updating predictable.

---

# 13. Game Launching

When the user presses:

```text
ENTER
```

RustArcade should:

```text
Save current TUI state
        ↓
Leave alternate terminal screen
        ↓
Restore terminal
        ↓
Start game process
        ↓
Wait for game to exit
        ↓
Restore RustArcade terminal mode
        ↓
Return to previous screen
```

This is critical.

Running the child process while Ratatui still owns the terminal can corrupt the interface.

---

# 14. Launch Lifecycle

Pseudo-flow:

```rust
disable_raw_mode();
leave_alternate_screen();

let status = Command::new(game.executable)
    .args(&game.args)
    .status();

enter_alternate_screen();
enable_raw_mode();
redraw();
```

RustArcade should also properly handle:

```text
CTRL+C
SIGTERM
Game crashes
Terminal resize
Unexpected child exit
```

---

# 15. User Experience

Main screen:

```text
┌──────────────────────────────────────────────────────────────┐
│                        RUSTARCADE                            │
├──────────────────────────────────────────────────────────────┤
│ Home   Library   Discover   Favorites   Updates   Settings   │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  Featured Games                                              │
│                                                              │
│  > Chess TUI                 Strategy       Installed        │
│    Tetro TUI                 Puzzle         Install          │
│    Rebels in the Sky         Sports         Install          │
│    BalatroTUI                Cards          Update           │
│    Sudoku TUI                Puzzle         Install          │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│ ↑↓ Navigate   Enter Select   / Search   F Favorite   Q Quit  │
└──────────────────────────────────────────────────────────────┘
```

---

# 16. Game Details Screen

```text
┌──────────────────────────────────────────────────────────────┐
│ Chess TUI                                                    │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│ Play chess in any terminal.                                  │
│                                                              │
│ Repository                                                   │
│ thomas-mauran/chess-tui                                      │
│                                                              │
│ Categories                                                   │
│ Chess · Strategy · Board                                     │
│                                                              │
│ Platform                                                     │
│ Linux ✓   macOS ✓   Windows ✓                                │
│                                                              │
│ Installation                                                 │
│ GitHub Release                                               │
│                                                              │
│ Status                                                       │
│ Not Installed                                                │
│                                                              │
│                 [ INSTALL & PLAY ]                           │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

---

# 17. Installation Confirmation

Before any installation:

```text
Install Chess TUI?

Source:
github.com/thomas-mauran/chess-tui

Installation method:
GitHub Release

Destination:
~/.local/share/rustarcade/games/chess-tui

System changes:
No administrator privileges required.

[Install] [Cancel]
```

For installations that require compiling:

```text
This game requires Rust/Cargo.

Cargo was found:
~/.cargo/bin/cargo

RustArcade will compile the game locally.

[Continue]
```

---

# 18. Dependency Detection

RustArcade should detect tools using PATH.

Examples:

```text
git
cargo
rustc
curl
tar
unzip
```

Prefer Rust-native HTTP downloads over requiring `curl`.

Recommended dependency policy:

```text
HTTP        → reqwest
Archives    → Rust crates
Git         → git executable or git2
Cargo       → external cargo executable
```

---

# 19. Missing Dependencies

If Cargo is missing:

```text
Cargo is required to install this game.

RustArcade will not install system dependencies automatically.

Suggested command:

Linux/macOS:
rustup toolchain install stable

Visit:
https://rustup.rs/
```

RustArcade should not automatically run:

```bash
sudo apt install ...
```

unless such behavior is eventually added as an explicit opt-in feature.

---

# 20. Update System

RustArcade should determine whether updates exist.

For GitHub release installations:

```text
installed version
      ↓
GitHub latest release
      ↓
semver comparison
      ↓
Update Available
```

For Cargo:

```text
installed version
      ↓
crate latest version
      ↓
semver comparison
```

TUI:

```text
UPDATES

Chess TUI
0.8.1 → 0.8.2

Tetro TUI
1.3.0 → 1.4.0

[U] Update selected
[A] Update all
```

---

# 21. Uninstall System

RustArcade must know exactly which files it owns.

Recommended behavior:

```text
Uninstall Chess TUI?

This will remove:

~/.local/share/rustarcade/games/chess-tui
~/.local/share/rustarcade/bin/chess-tui

Game save/config directories outside RustArcade will NOT be removed.

[Uninstall] [Cancel]
```

Do not automatically delete third-party save data.

---

# 22. Catalog Architecture

Use two catalogs.

## Built-In Catalog

Shipped with RustArcade.

```text
catalog/games/
```

Provides an offline baseline.

---

## Remote Catalog

Hosted in the RustArcade GitHub repository.

Example:

```text
https://raw.githubusercontent.com/<org>/RustArcade/main/catalog/index.json
```

At startup:

```text
Load local catalog
      ↓
Load cached remote catalog
      ↓
Optionally check for newer catalog
      ↓
Validate
      ↓
Merge
```

RustArcade should remain functional offline.

---

# 23. Catalog Index

Example:

```json
{
  "schema_version": 1,
  "updated_at": "2026-08-27T00:00:00Z",
  "games": [
    "chess-tui.toml",
    "tetro-tui.toml",
    "balatro-tui.toml"
  ]
}
```

---

# 24. Initial Catalog

The first RustArcade catalog can be based on the previously identified terminal/TUI projects.

Suggested launch selection:

```text
Chess TUI
Tetro TUI
Rebels in the Sky
BalatroTUI
2048 TUI
Connect Four
Sudoku TUI
SnakeShell
Tetrust
Terminal Poker
Mastermind
Battleship
Game of Life
TermFarm
TMaze
```

Do not attempt to support all repositories immediately.

Start with approximately:

```text
10–15 fully verified games
```

and expand after the installation system is stable.

---

# 25. Compatibility Classification

Each catalog entry should have one of these statuses:

```text
Verified
Community Tested
Experimental
Broken
Archived
```

Example:

```toml
support_status = "verified"
```

---

# 26. Catalog Contribution Workflow

Community contribution:

```text
Fork RustArcade
     ↓
Add game manifest
     ↓
Run validator
     ↓
Open Pull Request
     ↓
CI validates manifest
     ↓
Maintainer review
     ↓
Merge
```

Example:

```bash
rustarcade catalog validate catalog/games/my-game.toml
```

---

# 27. Catalog Security Rules

Every catalog PR should automatically check:

- repository URL is valid;
- repository uses HTTPS;
- repository exists;
- manifest schema is valid;
- game ID is unique;
- executable name is valid;
- no arbitrary shell command exists;
- release host is trusted;
- installer type is supported;
- URLs do not use unsafe protocols;
- checksums are properly formatted;
- path traversal is impossible.

---

# 28. Security Model

RustArcade installs third-party software.

Therefore security must be treated as a core feature.

## Trust Boundary

```text
RustArcade
   │
   ├── trusted RustArcade catalog
   │
   └── untrusted third-party game code
```

Even if RustArcade itself is secure, games can still be malicious.

The UI should clearly show:

```text
Third-party software

RustArcade does not audit or sandbox every game.
Installing a game executes software from its original developer.
```

---

# 29. Download Integrity

Where available:

```text
SHA-256
```

should be verified.

Example manifest:

```toml
[[installers]]
type = "github-release"
asset_pattern = "game-linux-x86_64.tar.gz"
sha256 = "..."
```

For dynamically changing releases, RustArcade can retrieve checksum files only from trusted release metadata.

---

# 30. Optional Future Sandboxing

Possible future Linux support:

```text
bubblewrap
flatpak-spawn
systemd-run
```

Possible options:

```text
Disable network
Read-only home
Separate save directory
Limit filesystem access
```

This should be a later feature, not MVP.

---

# 31. Local Configuration

Recommended config:

```text
~/.config/rustarcade/config.toml
```

Example:

```toml
theme = "default"

auto_update_catalog = true
check_game_updates = true

confirm_before_install = true
confirm_before_update = true

show_experimental_games = false

[paths]
games = "~/.local/share/rustarcade/games"
```

---

# 32. Library State

Example:

```text
~/.local/share/rustarcade/state/library.json
```

Contains:

```json
{
  "favorites": [
    "chess-tui",
    "tetro-tui"
  ],
  "recent": [
    "chess-tui"
  ]
}
```

---

# 33. Play History

RustArcade can record:

```text
Game
Launch time
Exit time
Duration
Exit code
```

Example:

```json
{
  "game": "chess-tui",
  "started_at": "2026-08-27T20:00:00Z",
  "duration_seconds": 1820,
  "exit_code": 0
}
```

Possible UI:

```text
PLAY STATS

Chess TUI        8h 42m
Tetro TUI        3h 15m
Sudoku TUI       1h 04m
```

---

# 34. CLI Interface

RustArcade should also provide non-TUI commands.

```bash
rustarcade
```

Launch TUI.

Additional commands:

```bash
rustarcade list
rustarcade search chess
rustarcade info chess-tui

rustarcade install chess-tui
rustarcade play chess-tui
rustarcade update chess-tui
rustarcade update --all
rustarcade uninstall chess-tui

rustarcade catalog update
rustarcade catalog validate

rustarcade doctor
rustarcade version
```

The CLI and TUI should call the same internal application services.

---

# 35. Doctor Command

Very useful for troubleshooting.

```bash
rustarcade doctor
```

Example:

```text
RustArcade Doctor

OS                 Linux ✓
Architecture       x86_64 ✓
Terminal           xterm-256color ✓

Git                2.48.1 ✓
Cargo              1.92.0 ✓
Rustc               1.92.0 ✓

Data directory     writable ✓
Config directory   writable ✓
Catalog cache      valid ✓

GitHub              reachable ✓

No problems found.
```

---

# 36. Logging

Logs:

```text
~/.local/state/rustarcade/logs/
```

Use:

```text
tracing
tracing-subscriber
```

Do not show verbose technical logs inside the normal TUI.

Provide:

```bash
rustarcade --debug
```

---

# 37. Error Handling

Errors should be actionable.

Bad:

```text
Installation failed.
```

Good:

```text
Unable to install Tetro TUI.

Cargo returned exit code 101.

Possible causes:
• Rust toolchain is outdated
• Required native library is missing

Log:
~/.local/state/rustarcade/logs/install-tetro-tui.log

[R] Retry
[L] View Log
[B] Back
```

---

# 38. MVP Scope

The MVP should contain only the features required to prove the product.

## MVP v0.1

Must have:

- Rust CLI application;
- Ratatui interface;
- local catalog;
- game list;
- game details;
- OS detection;
- architecture detection;
- installed/not-installed detection;
- Cargo installer;
- GitHub release installer;
- install confirmation;
- game launching;
- return to RustArcade after exit;
- uninstall;
- basic logs;
- approximately 10 verified games.

Do not include yet:

- user accounts;
- multiplayer services;
- cloud saves;
- achievements;
- friends;
- ratings;
- automatic system package installation;
- advanced sandboxing.

---

# 39. Development Roadmap

## Phase 1 — Foundation

Estimated focus:

```text
Project structure
Core types
Error system
Configuration
Storage paths
CLI skeleton
```

Deliverable:

```bash
rustarcade --version
rustarcade doctor
```

---

## Phase 2 — Catalog

Implement:

```text
GameManifest
Catalog loader
TOML parser
Manifest validator
Search/filter
```

Deliverable:

```bash
rustarcade list
rustarcade info chess-tui
```

---

## Phase 3 — Installation

Implement:

```text
Installer trait
Cargo installer
GitHub release installer
Installation registry
Uninstaller
```

Deliverable:

```bash
rustarcade install chess-tui
```

---

## Phase 4 — Launcher

Implement:

```text
Executable resolution
Terminal restoration
Child process execution
Return to launcher
Play history
```

Deliverable:

```bash
rustarcade play chess-tui
```

---

## Phase 5 — TUI

Implement:

```text
Home screen
Catalog
Game details
Library
Install progress
Error dialogs
Keyboard navigation
```

Deliverable:

```bash
rustarcade
```

---

## Phase 6 — Updates

Implement:

```text
Catalog updates
Game update detection
Update installation
```

---

## Phase 7 — Production Hardening

Implement:

```text
Checksums
Security validation
CI
Cross-platform builds
Packaging
Documentation
Catalog contribution workflow
```

---

# 40. Step-by-Step Implementation Plan

## Step 1 — Create the Repository

```bash
cargo new rustarcade
cd rustarcade
```

Initialize Git:

```bash
git init
```

Create:

```text
README.md
LICENSE
SECURITY.md
CONTRIBUTING.md
```

---

## Step 2 — Add Core Dependencies

Start with:

```toml
[dependencies]
anyhow = "1"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
crossterm = "0.29"
directories = "6"
ratatui = "0.29"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
semver = { version = "1", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
toml = "0.9"
tracing = "0.1"
tracing-subscriber = "0.3"
url = "2"
which = "8"
```

Version numbers should be reviewed before finalizing the actual implementation.

---

# 41. Step 3 — Implement Platform Detection

Create:

```text
src/platform/
```

Detect:

```rust
std::env::consts::OS
std::env::consts::ARCH
```

Normalize values:

```text
linux
macos
windows

x86_64
aarch64
```

---

# 42. Step 4 — Implement Application Paths

Using `directories`:

```text
Config
Data
Cache
State
```

RustArcade should create directories on first run.

Example Linux paths:

```text
~/.config/rustarcade/
~/.local/share/rustarcade/
~/.cache/rustarcade/
~/.local/state/rustarcade/
```

---

# 43. Step 5 — Define GameManifest

Rust model:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct GameManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub repository: String,
    pub category: Vec<String>,
    pub compatibility: Compatibility,
    pub run: RunConfig,
    pub installers: Vec<InstallerConfig>,
}
```

---

# 44. Step 6 — Implement Manifest Validation

Reject manifests with:

```text
Invalid IDs
Unknown installer type
Invalid repository URL
Unsupported schema version
Empty run command
Duplicate game ID
Unsafe paths
Shell snippets
```

---

# 45. Step 7 — Create Initial Catalog

Start with only games whose installation process is understood.

Suggested first batch:

```text
chess-tui
tetro-tui
game-2048-tui
sudoku-tui
snakeshell
tetrust
termfarm
mastermind-rs
terminal-poker
battleship-rs
```

Verify each manually before marking:

```text
Verified
```

---

# 46. Step 8 — Create Installer Trait

Example concept:

```rust
trait Installer {
    fn is_supported(&self, game: &GameManifest) -> bool;
    async fn install(&self, game: &GameManifest) -> Result<Installation>;
    async fn update(&self, game: &GameManifest) -> Result<Installation>;
    async fn uninstall(&self, installation: &Installation) -> Result<()>;
}
```

---

# 47. Step 9 — Implement Cargo Installer

Workflow:

```text
Check cargo exists
      ↓
Read crate name
      ↓
Create isolated install root
      ↓
cargo install
      ↓
Verify binary
      ↓
Register installation
```

Prefer:

```bash
cargo install --root <rustarcade-game-dir> <crate>
```

instead of polluting the user's global Cargo installation.

Example:

```bash
cargo install \
  --root ~/.local/share/rustarcade/games/chess-tui \
  chess-tui
```

---

# 48. Step 10 — Implement GitHub Release Installer

Use GitHub API.

Workflow:

```text
GET latest release
     ↓
Parse assets
     ↓
Match OS + architecture
     ↓
Download
     ↓
Verify
     ↓
Extract
     ↓
Find executable
     ↓
Register
```

Cache metadata to avoid unnecessary requests.

---

# 49. Step 11 — Implement Installation Registry

Create:

```text
installed.json
```

RustArcade should be able to answer:

```text
Is installed?
Which version?
Where is executable?
Which installer was used?
When installed?
```

---

# 50. Step 12 — Implement Launcher

Create a single launcher service.

Interface:

```rust
launch(game_id)
```

Responsibilities:

```text
Resolve installation
Verify executable exists
Suspend TUI
Launch child process
Wait
Restore terminal
Record result
```

---

# 51. Step 13 — Build the TUI Navigation

Recommended screens:

```text
Home
Discover
Library
Favorites
Updates
Settings
Game Details
```

Keyboard:

```text
↑ / k        Up
↓ / j        Down
Enter        Select
Esc          Back
/            Search
F            Favorite
I            Install
U            Update
X            Uninstall
Q            Quit
```

---

# 52. Step 14 — Installation Progress UI

Example:

```text
Installing Tetro TUI

[██████████████████░░░░░░] 72%

Downloading source...
Compiling...

Elapsed: 00:34

Please wait.
```

Installer work must run asynchronously so the TUI remains responsive.

---

# 53. Step 15 — Add Favorites and History

Save:

```text
favorite game IDs
recent games
play duration
```

Then create Home sections:

```text
Continue Playing
Favorites
Recently Installed
Recently Played
```

---

# 54. Step 16 — Implement Updates

Create:

```bash
rustarcade update
```

and:

```text
Updates screen
```

Never update while a game is currently running.

---

# 55. Step 17 — Add Catalog Validation CLI

```bash
rustarcade catalog validate
```

CI should run this for every PR.

---

# 56. Step 18 — Add Automated Tests

Minimum test categories:

## Unit

```text
Manifest parsing
Manifest validation
OS matching
Architecture matching
Release asset selection
Path safety
Version comparison
```

## Integration

```text
Install fixture binary
Launch fixture game
Capture exit code
Uninstall fixture
```

## TUI

Test state transitions rather than terminal rendering wherever possible.

---

# 57. Step 19 — GitHub Actions CI

Test:

```text
Linux
macOS
Windows
```

Pipeline:

```text
cargo fmt --check
cargo clippy
cargo test
cargo build --release
catalog validation
```

---

# 58. Step 20 — Release Pipeline

Create binaries for:

```text
linux-x86_64
linux-aarch64
macos-x86_64
macos-aarch64
windows-x86_64
```

Publish to GitHub Releases.

---

# 59. RustArcade Installation

Target user experience:

Linux/macOS:

```bash
cargo install rustarcade
```

Later:

```bash
curl -fsSL https://rustarcade.dev/install.sh | sh
```

If an install script is eventually provided, it should be hosted and documented carefully.

Other future options:

```text
Homebrew
AUR
Scoop
Winget
Nix
Flatpak
```

---

# 60. Recommended First-Run Experience

```text
Welcome to RustArcade

RustArcade lets you discover and launch terminal games
from their original open-source repositories.

RustArcade does not bundle games.

Before installation you will always be shown:
• repository
• installation method
• destination
• required dependencies

[Continue]
```

Then:

```text
System Check

Linux x86_64                  ✓
Git                           ✓
Cargo                         ✓
Internet                      ✓

Everything is ready.

[Enter RustArcade]
```

---

# 61. Suggested Main Menu

```text
RUSTARCADE

> Play
  Discover
  Library
  Favorites
  Updates
  Settings

10 games available
3 installed
1 update available
```

---

# 62. Recommended Game Statuses

Use simple states:

```text
AVAILABLE
INSTALLING
INSTALLED
UPDATE AVAILABLE
RUNNING
BROKEN
UNSUPPORTED
```

---

# 63. Future Features

After a stable v1.0:

## Gamepad Support

Navigate RustArcade using:

```text
D-pad
A
B
X
Y
Start
```

---

## SSH Mode

Potential:

```bash
ssh play@rustarcade.example
```

Users could browse a remote RustArcade instance.

This requires a different execution model because remote users should not receive unrestricted server access.

---

## Community Catalogs

Allow additional sources:

```toml
[[catalogs]]
name = "official"
url = "..."

[[catalogs]]
name = "community"
url = "..."
```

Third-party catalogs should show an explicit trust warning.

---

## Themes

Example:

```text
Default
Retro Green
Amber CRT
Cyberpunk
Monochrome
```

---

## Game Screenshots

Potential future support for:

```text
ANSI previews
ASCII screenshots
terminal recordings
```

---

## Ratings

Avoid requiring a RustArcade server initially.

Possible future integration:

```text
GitHub stars
Community metadata
Local personal ratings
```

---

# 64. Version Milestones

## v0.1 — Proof of Concept

```text
Catalog
Cargo installs
Launch
Return to launcher
5 games
```

## v0.2 — Installer Platform

```text
GitHub releases
Installation registry
Uninstall
10+ games
```

## v0.3 — Full TUI

```text
Discover
Library
Favorites
Search
Game details
```

## v0.4 — Updates

```text
Catalog updates
Game updates
Update screen
```

## v0.5 — Cross Platform

```text
Linux
macOS
Windows
CI releases
```

## v1.0 — Stable

```text
Stable manifest format
Security model
Verified catalog
Documentation
Contribution workflow
Production release pipeline
```

---

# 65. Definition of Done for v1.0

RustArcade v1.0 should be considered complete when:

- [ ] RustArcade launches reliably on Linux, macOS, and Windows.
- [ ] The TUI works correctly after child games exit.
- [ ] The official catalog contains at least 20 verified games.
- [ ] Game manifests are declarative and schema validated.
- [ ] No catalog entry can execute arbitrary shell commands.
- [ ] GitHub Release installations work.
- [ ] Cargo installations work.
- [ ] Games are installed in RustArcade-managed directories.
- [ ] Installed games can be updated.
- [ ] Installed games can be uninstalled.
- [ ] Favorites work.
- [ ] Search works.
- [ ] Play history works.
- [ ] The Doctor command works.
- [ ] Logs are available for failed installations.
- [ ] CI covers all supported platforms.
- [ ] Release binaries are automatically generated.
- [ ] SECURITY.md documents the third-party software trust model.
- [ ] CONTRIBUTING.md explains how to add a game.
- [ ] Catalog PRs are automatically validated.

---

# 66. Recommended Development Priority

The project should be developed in this order:

```text
1. Core domain
2. Catalog
3. Installer
4. Launcher
5. Installation registry
6. TUI
7. Updates
8. Security hardening
9. Cross-platform support
10. Catalog expansion
```

Do **not** begin by building the visual TUI first.

The hardest and most important component is:

```text
Catalog → Install → Verify → Launch → Return
```

Once that pipeline works reliably, the TUI becomes the user-friendly interface around it.

---

# 67. Core Domain Objects

Recommended primary models:

```text
GameManifest
GameCompatibility
InstallerConfig
Installation
GameStatus
Catalog
LaunchResult
PlaySession
RustArcadeConfig
PlatformInfo
```

Avoid coupling UI widgets directly to filesystem or installer logic.

---

# 68. Best Architectural Rule

Every user action should travel through an application service.

Example:

```text
TUI
 ↓
InstallGameAction
 ↓
GameManager
 ↓
InstallerManager
 ↓
Installer Provider
```

Not:

```text
TUI button
 ↓
Command::new("cargo")
```

This separation will make the CLI, TUI, tests, and future APIs share the same core.

---

# 69. Suggested Internal API

Conceptually:

```rust
game_manager.list()
game_manager.search(query)
game_manager.info(id)

game_manager.install(id)
game_manager.launch(id)
game_manager.update(id)
game_manager.uninstall(id)

game_manager.favorite(id)
game_manager.unfavorite(id)
```

Both:

```bash
rustarcade install chess-tui
```

and the TUI's Install button should call:

```text
game_manager.install("chess-tui")
```

---

# 70. Recommended Repository Strategy

Keep the catalog inside the main RustArcade repository initially.

```text
RustArcade
├── application
└── catalog
```

Advantages:

- one PR process;
- one CI pipeline;
- easy releases;
- catalog format can evolve with the application.

If the catalog grows significantly, it can later move to:

```text
rustarcade/catalog
```

as a separate repository.

---

# 71. Recommended GitHub Repository Description

> A Rust-powered terminal game launcher. Discover, install, update, and play open-source TUI and CLI games directly from one terminal interface.

---

# 72. Suggested README Tagline

```text
Your terminal. Your arcade.
```

Alternative:

```text
One terminal. Hundreds of games.
```

---

# 73. Final Product Flow

The ideal RustArcade experience:

```text
$ rustarcade
```

User sees:

```text
RUSTARCADE
```

Selects:

```text
Tetro TUI
```

RustArcade says:

```text
Not installed.

Install from:
github.com/Strophox/tetro-tui

Method:
GitHub Release

[Install & Play]
```

User presses Enter.

RustArcade:

```text
Downloading...
Installing...
Verifying...

✓ Ready
```

RustArcade temporarily exits its TUI.

The game launches normally in the terminal.

When the game exits:

```text
RustArcade
```

returns immediately to:

```text
Tetro TUI

Last played: Just now
Play time: 23 minutes

[Play Again]
```

That is the core experience the entire architecture should protect.

---

# 74. Final Recommendation

The strongest implementation strategy for RustArcade is:

```text
Rust binary
+
Ratatui interface
+
Declarative TOML game catalog
+
Structured installer providers
+
RustArcade-managed game directories
+
Safe child-process launcher
+
GitHub-hosted community catalog
```

The most important design principle is:

> **RustArcade should understand installation methods, not arbitrary installation commands.**

That makes the project significantly safer, easier to test, easier to maintain, and much easier for the community to extend.

Once the installation and launch pipeline is stable, RustArcade can grow from a simple launcher into a complete open-source terminal gaming platform without changing its fundamental architecture.
