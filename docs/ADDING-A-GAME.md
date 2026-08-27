# Adding a Game to the Catalog

A catalog entry is one TOML file in `catalog/games/`, named after the game id. This document
lists every field, the validation rules, and the research checklist used for the launch
catalog.

## Research checklist

Before writing a manifest, confirm from the project itself (Cargo.toml, release page, CI):

1. It is a terminal game (TUI or interactive CLI) that runs in a single foreground process.
   Games that need a separate server process or a GUI cannot be launched by RustArcade.
2. The repository is public and reachable over HTTPS; note whether it is archived.
3. The license (SPDX id where declared; say "README-declared" if there is no license file).
4. The executable name produced by the build (`[[bin]]` name or package name).
5. Installation paths that actually work:
   - GitHub releases: asset naming pattern per platform, archive layout, checksum files.
   - crates.io: crate name and whether it has a binary target (`cargo info <crate>`).
   - Git build: does `cargo build --release` work without extra system libraries?
6. Native dependencies (ALSA, OpenSSL, cmake, a C compiler) and runtime requirements
   (external engines, minimum terminal size).
7. Supported operating systems and architectures.

Then install and launch it through RustArcade in an isolated home:

```bash
RUSTARCADE_HOME=/tmp/ra-test cargo run -- install <id> --yes --play
```

## Manifest reference

```toml
schema_version = 1                 # required, must be 1
id = "chess-tui"                   # required; ^[a-z0-9]+(-[a-z0-9]+)*$, 2-48 chars, equals file name
name = "Chess TUI"                 # required, ≤ 64 chars
summary = "One line shown in lists."  # required, ≤ 160 chars
description = """Optional longer text."""
repository = "https://github.com/owner/repo"   # required, https only, no credentials/query
homepage = "https://…"             # optional, https only
license = "MIT"                    # optional free text (SPDX id recommended)
authors = ["Name"]                 # optional
categories = ["board", "strategy"] # required, 1-5 of: arcade puzzle board card strategy roguelike
                                   #   simulation sports typing idle classic multiplayer other
tags = ["chess"]                   # optional lowercase slugs
support_status = "verified"        # verified | community-tested | experimental | broken | archived
verified_on = "2026-08-27"         # optional YYYY-MM-DD
notes = "Shown on the details screen."   # optional

[compatibility]
os = ["linux", "macos", "windows"] # required, non-empty
arch = ["x86_64", "aarch64"]       # optional; omitted = all
min_terminal = { cols = 80, rows = 24 }   # optional hint

[requirements]                     # optional
commands = ["stockfish"]           # external programs required at runtime
optional_commands = ["gnugo"]      # programs that unlock optional features
notes = "Free text shown in the install plan"

[run]
executable = "chess-tui"           # required bare file name, no directories, no .exe
args = ["--no-sound"]              # optional argv (never interpreted by a shell)
env = { GAME_MODE = "tui" }        # optional; PATH, HOME, LD_*, DYLD_* are forbidden
cwd = "current"                    # current (default) | install | home
```

### Installers

One to six `[[installers]]` entries in order of preference. Every kind accepts:

- `os = [...]` — restrict to a subset of `compatibility.os`. Two installers of the same kind
  must not overlap in OS (this is how per-OS feature flags are expressed).
- `binary = "name"` or `"dir/name"` — where the executable lives inside the build/archive when
  it differs from `run.executable` (relative, no `..`, ≤ 4 components).
- `warnings = ["…"]` — shown in the install plan.

**GitHub release**

```toml
[[installers]]
type = "github-release"
repository = "owner/repo"
asset = "game-{version}-{target}.{ext}"   # optional pattern; placeholders:
                                          #   {version} 1.2.3   {tag} v1.2.3   {os} linux|macos|windows
                                          #   {arch} x86_64|aarch64   {target} rust triple   {ext} tar.gz|tgz|tar.xz|txz|zip|exe
checksum_asset = "{asset}.sha256"         # optional checksum file ({asset} = chosen asset name)
tag = "v1.2.3"                            # optional pin (required when sha256 is used)
allow_prerelease = false
[installers.asset_patterns]               # optional per-platform patterns (win over `asset`)
linux-x86_64 = "game_Linux_x86_64"
[installers.sha256]                       # optional pinned digests per asset name
"game-1.2.3-x86_64-unknown-linux-gnu.tar.gz" = "…64 hex…"
```

Without a pattern, assets are matched heuristically by OS and architecture tokens (checksum,
signature, source and package files are ignored). Checksums are taken from, in order: the
`sha256` table, the digest GitHub publishes for the asset, the checksum file. Raw (non-archive)
assets are supported.

**Cargo**

```toml
[[installers]]
type = "cargo"
crate = "chess-tui"
version = "^2"            # optional semver requirement
features = ["extra"]      # optional
default_features = true   # optional
locked = false            # optional → --locked
bins = ["chess-tui"]      # optional → --bin, for crates that ship several executables
```

**Git + Cargo build**

```toml
[[installers]]
type = "git-cargo-build"
repository = "https://github.com/owner/repo"
reference = "v1.2.3"      # optional tag or branch
commit = "…40 hex…"       # optional: the checkout must resolve to this commit
path = "crates/game"      # optional package directory inside the repository
package = "game"          # optional -p for workspaces
features = [] ; default_features = true ; locked = false ; bins = []
```

## What is rejected

Unknown fields (including anything that looks like a command), non-HTTPS URLs, unknown
installer types, unsupported schema versions, invalid ids, duplicate ids, duplicate executables,
paths with `..` or absolute paths, `.exe` suffixes, invalid semver requirements, malformed
digests, unknown pattern placeholders, forbidden environment variables, and installers whose
`os` is not a subset of `compatibility.os`.

Run `cargo run -- catalog validate catalog/games/<id>.toml` to see every problem with its
field path, then `cargo run -- catalog index` to refresh `catalog/index.json`.

## Candidates evaluated but not included

| Project | Reason |
|---|---|
| scottnm/tetrust | Depends on a git repository that no longer exists; cannot be built |
| Cod-e-Codes/battleship-rs | Requires a separate server process (`server-ai`) before the client can run |
| joshhansen/Umpire | Current sources do not build from a clean clone; old crate needs audio dev libraries |
