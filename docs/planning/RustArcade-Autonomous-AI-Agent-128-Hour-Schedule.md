# RustArcade — Autonomous AI Agent Execution Schedule
## From an Empty Folder to a Complete, Tested, Documented, GitHub-Ready Product

> **Target:** Build RustArcade completely from scratch, starting from an empty directory, with no work required from the user.
>
> **Execution model:** The AI agent is expected to make reasonable technical decisions autonomously, research when necessary, implement the product, test it, fix failures, document everything, and leave the repository in a clean state ready for a GitHub commit.
>
> **Estimated implementation budget:** **128 focused engineering hours**
>
> This schedule is intentionally detailed and includes architecture, implementation, catalog work, security hardening, automated testing, documentation, CI/CD, release preparation, and final repository cleanup.

---

# 1. Mission

Build **RustArcade**, a Rust-powered terminal game launcher that lets users browse a curated TUI catalog, automatically download/install supported third-party terminal games, launch them in the terminal, and return cleanly to RustArcade when the game exits.

RustArcade must **not bundle the games themselves**.

The final repository must be:

- complete;
- functional;
- tested;
- documented;
- secure by design;
- maintainable;
- cross-platform where reasonably possible;
- ready for a single clean GitHub commit;
- usable without the user performing implementation work.

---

# 2. Non-Negotiable Autonomous-Agent Rules

The AI agent must follow these rules for the entire project.

## The user does not participate in implementation

The agent must **not** stop to ask the user to:

- create files;
- install dependencies manually;
- write code;
- choose routine implementation details;
- run tests;
- fix compiler errors;
- format files;
- create documentation;
- investigate failures;
- prepare Git;
- clean the repository.

The agent owns those tasks.

---

## Resolve ordinary ambiguity autonomously

When multiple reasonable technical choices exist:

1. choose the safest maintainable option;
2. document the decision;
3. continue.

Only block when there is a genuine external impossibility such as:

- unavailable credentials that are strictly required;
- inaccessible private infrastructure;
- an operation that cannot legally or technically be performed without external authorization.

The project itself should be designed so that no credentials are needed for normal development.

---

## Never leave known failures behind

Before declaring completion, the agent must resolve:

- compiler errors;
- failed tests;
- formatting issues;
- Clippy warnings that indicate actionable code quality problems;
- broken manifests;
- missing documentation;
- dead references;
- unsafe arbitrary-shell execution paths;
- untracked temporary files;
- accidental secrets;
- build artifacts committed by mistake.

---

# 3. Definition of Final Completion

At the end of Hour 128, the repository should satisfy all of the following.

- [ ] Rust project exists and builds successfully.
- [ ] Ratatui interface is functional.
- [ ] CLI interface is functional.
- [ ] Official game catalog is implemented.
- [ ] Declarative game manifests are schema/version validated.
- [ ] At least 10 initial games are integrated and manually verified as far as the environment allows.
- [ ] Cargo-based installation works.
- [ ] GitHub Release installation works.
- [ ] Installed-game registry works.
- [ ] Install / launch / return-to-TUI workflow works.
- [ ] Update detection works.
- [ ] Uninstallation works.
- [ ] Favorites work.
- [ ] Search and filtering work.
- [ ] Play history works.
- [ ] Doctor diagnostics work.
- [ ] Security protections are implemented.
- [ ] Arbitrary shell snippets are not accepted by official manifests.
- [ ] Unit tests pass.
- [ ] Integration tests pass.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy` passes with the configured policy.
- [ ] `cargo test` passes.
- [ ] Release build succeeds.
- [ ] CI workflows exist.
- [ ] Release workflow exists.
- [ ] README is complete.
- [ ] CONTRIBUTING.md exists.
- [ ] SECURITY.md exists.
- [ ] LICENSE exists.
- [ ] CHANGELOG.md exists.
- [ ] Architecture documentation exists.
- [ ] Catalog contribution documentation exists.
- [ ] No secrets exist in the repository.
- [ ] `.gitignore` is complete.
- [ ] Working tree is clean except for intentionally staged/uncommitted project files.
- [ ] Repository is ready for `git add . && git commit`.

---

# 4. Total Timeline

| Phase | Hours | Focus |
|---|---:|---|
| 1 | 0–6 | Research, bootstrap, requirements freeze |
| 2 | 6–16 | Repository architecture and core foundation |
| 3 | 16–30 | Catalog domain, manifests, validation |
| 4 | 30–48 | Installation engine |
| 5 | 48–60 | Game launcher and terminal lifecycle |
| 6 | 60–78 | Full TUI |
| 7 | 78–88 | CLI, configuration, storage, diagnostics |
| 8 | 88–98 | Updates, uninstall, recovery |
| 9 | 98–108 | Security hardening |
| 10 | 108–116 | Initial verified game catalog |
| 11 | 116–123 | Testing, CI/CD, release preparation |
| 12 | 123–128 | Documentation, final QA, GitHub-ready cleanup |

**Total: 128 hours**

---

# 5. Phase 1 — Research, Bootstrap, and Requirements Freeze
## Hours 0–6

### Hour 0–1 — Initialize the empty workspace

Starting condition:

```text
./
```

Create the project directory structure and initialize:

```bash
cargo init
git init
```

Create foundational files:

```text
Cargo.toml
README.md
LICENSE
.gitignore
rustfmt.toml
clippy.toml
```

Establish the initial package name:

```text
rustarcade
```

Expected result:

```bash
cargo check
```

must succeed.

---

### Hour 1–2 — Validate the current Rust ecosystem

Research current stable versions and compatibility for the planned crates.

Evaluate:

```text
ratatui
crossterm
tokio
reqwest
serde
toml
clap
directories
semver
sha2
thiserror
anyhow
tracing
tracing-subscriber
which
tempfile
```

Avoid blindly copying obsolete dependency versions.

Document major dependency choices.

---

### Hour 2–3 — Freeze the MVP and v1.0 scope

Create:

```text
docs/PRODUCT-SPEC.md
```

Define:

- product purpose;
- user flows;
- non-goals;
- supported installer types;
- supported platforms;
- catalog trust model;
- TUI screens;
- CLI commands;
- installation behavior;
- update behavior;
- uninstall behavior.

No implementation should proceed with ambiguous core behavior.

---

### Hour 3–4 — Define the architecture

Create:

```text
docs/ARCHITECTURE.md
```

Define clear components:

```text
App
Catalog
Game Manager
Installer Manager
Installer Providers
Launcher
Platform Detector
Storage
TUI
CLI
GitHub Client
Security Validator
```

Ensure the TUI does not directly execute installation commands.

---

### Hour 4–5 — Threat model

Create:

```text
docs/THREAT-MODEL.md
```

Analyze:

- malicious manifests;
- command injection;
- path traversal;
- compromised GitHub repositories;
- archive traversal;
- checksum mismatch;
- executable substitution;
- unsafe URLs;
- arbitrary shell execution;
- symlink abuse;
- overwrite of user files.

Define defenses before installer development.

---

### Hour 5–6 — Create implementation backlog

Create:

```text
docs/IMPLEMENTATION-CHECKLIST.md
```

Translate this schedule into actionable checkboxes.

**Phase Gate**

Run:

```bash
cargo fmt --check
cargo check
git status
```

Do not continue until the foundation is clean.

---

# 6. Phase 2 — Repository Architecture and Core Foundation
## Hours 6–16

### Hour 6–8 — Build the module structure

Create:

```text
src/
├── app/
├── catalog/
├── installer/
├── launcher/
├── platform/
├── storage/
├── github/
├── tui/
└── error.rs
```

Implement module boundaries without premature feature logic.

---

### Hour 8–10 — Error architecture

Implement typed application errors.

Recommended categories:

```text
CatalogError
ManifestError
InstallerError
LaunchError
StorageError
NetworkError
PlatformError
SecurityError
```

Use:

```text
thiserror
anyhow
```

appropriately.

Avoid string-only error handling.

---

### Hour 10–12 — Application paths

Implement platform-aware directories using `directories`.

Support:

```text
Config
Data
Cache
State
Logs
Games
Bin
Catalog cache
```

Ensure directory creation is idempotent.

Write unit tests.

---

### Hour 12–14 — Platform detection

Implement normalized detection for:

```text
Linux
macOS
Windows

x86_64
aarch64
```

Include terminal environment metadata where useful.

Write tests for normalization functions.

---

### Hour 14–16 — Core application state

Implement foundational domain models:

```text
GameId
GameStatus
PlatformInfo
Installation
InstalledGame
LaunchResult
PlaySession
AppConfig
```

Avoid leaking raw JSON/TOML structures throughout the application.

**Phase Gate**

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
```

---

# 7. Phase 3 — Catalog Domain, Manifests, and Validation
## Hours 16–30

### Hour 16–18 — Define manifest schema

Implement:

```rust
GameManifest
Compatibility
RunConfig
Requirements
InstallerConfig
```

Installer variants should be strongly typed.

Example conceptual enum:

```text
GithubRelease
Cargo
GitCargoBuild
```

Do not add arbitrary shell commands.

---

### Hour 18–20 — TOML manifest parser

Implement manifest loading from:

```text
catalog/games/*.toml
```

Support:

- schema version;
- clear validation errors;
- duplicate detection;
- deterministic loading.

---

### Hour 20–22 — Manifest validation

Validate:

- game ID format;
- repository URL;
- supported schema;
- executable name;
- platform values;
- architecture values;
- installer fields;
- destination-safe paths.

Reject unknown unsafe fields when appropriate.

---

### Hour 22–24 — Catalog abstraction

Implement:

```text
Catalog
CatalogEntry
CatalogLoader
CatalogIndex
```

Capabilities:

```text
list
get
search
filter
sort
```

---

### Hour 24–26 — Catalog cache

Implement local cache structure.

Support:

```text
built-in catalog
cached remote catalog
merged catalog
```

RustArcade must remain usable offline.

---

### Hour 26–28 — Remote catalog update logic

Implement remote index retrieval using Rust HTTP libraries.

Requirements:

- HTTPS only;
- timeout;
- cache;
- schema validation;
- graceful fallback;
- no startup failure when offline.

---

### Hour 28–30 — Catalog CLI foundation

Implement:

```bash
rustarcade list
rustarcade search <query>
rustarcade info <id>
rustarcade catalog validate
```

Add tests.

**Phase Gate**

All malformed test fixtures must fail safely.

---

# 8. Phase 4 — Installation Engine
## Hours 30–48

### Hour 30–32 — Installer abstraction

Create the installer provider interface.

Responsibilities:

```text
compatibility check
installation plan
install
version detection
update
uninstall metadata
```

The provider must receive structured arguments.

---

### Hour 32–34 — Installation planner

Before installing anything, generate:

```text
source
method
version
destination
required tools
network activity
expected executable
```

This plan will later be shown in the TUI.

---

### Hour 34–38 — Cargo installer

Implement isolated Cargo installations.

Preferred pattern:

```bash
cargo install --root <managed-directory> <crate>
```

Capabilities:

- detect Cargo;
- resolve crate/version;
- capture stdout/stderr;
- verify exit status;
- verify output executable;
- register installation.

Never execute through:

```bash
sh -c
```

---

### Hour 38–42 — GitHub Release installer

Implement:

```text
release lookup
asset selection
download
checksum support
archive extraction
binary discovery
permissions
registration
```

Match:

```text
OS
architecture
asset naming pattern
```

Add robust error messages for ambiguous assets.

---

### Hour 42–44 — Archive extraction safety

Support common archives:

```text
zip
tar.gz
tar.xz
```

Protect against:

```text
../ path traversal
absolute paths
unsafe symlinks
overwrite outside destination
```

Write security tests.

---

### Hour 44–46 — Installation registry

Implement persistent metadata:

```text
installed game ID
version
installer
binary path
source repository
install date
managed files
```

Use atomic writes.

---

### Hour 46–48 — Installation transaction and rollback

If installation fails halfway:

```text
remove partial installation
retain useful logs
do not register broken state
```

Implement installation-state transitions:

```text
Available
Installing
Installed
Broken
```

**Phase Gate**

At least one fixture must successfully install through Cargo-style logic or a local test substitute, and one fixture through release/archive logic.

---

# 9. Phase 5 — Game Launcher and Terminal Lifecycle
## Hours 48–60

### Hour 48–50 — Executable resolution

Implement safe executable lookup based on installation metadata.

Reject:

- missing executable;
- unexpected executable path;
- path outside managed installation unless explicitly allowed.

---

### Hour 50–53 — Terminal suspend/resume

Implement correct Ratatui/Crossterm lifecycle:

```text
save application state
disable raw mode
leave alternate screen
launch child
wait
re-enter alternate screen
enable raw mode
redraw
```

Ensure restoration also occurs after launch failures.

---

### Hour 53–55 — Process execution

Use direct process APIs.

Capture:

```text
exit code
signal/termination where supported
start time
end time
duration
```

The game must receive an interactive terminal.

---

### Hour 55–57 — Signal and crash handling

Test:

```text
Ctrl+C
child non-zero exit
missing binary
game crash
RustArcade interruption
```

The user's terminal must never be left in broken raw mode.

---

### Hour 57–60 — Play history

Persist:

```text
game
timestamp
duration
exit result
```

Create services for:

```text
recently played
total play time
last played
```

**Phase Gate**

The core flow must work:

```text
install → launch → exit → RustArcade resumes
```

---

# 10. Phase 6 — Full TUI
## Hours 60–78

### Hour 60–62 — TUI runtime

Implement:

```text
terminal initialization
event loop
input handling
resize events
render loop
application actions
```

Separate rendering from business logic.

---

### Hour 62–64 — Main layout

Create reusable:

```text
header
tabs
content panel
status bar
modal system
footer key hints
```

---

### Hour 64–66 — Home screen

Implement sections:

```text
Continue Playing
Recently Played
Favorites
Featured
Available Updates
```

Provide empty states.

---

### Hour 66–68 — Discover screen

Implement:

- catalog list;
- scrolling;
- search;
- category filtering;
- installed-state badges;
- compatibility indicators.

---

### Hour 68–70 — Game details screen

Show:

```text
name
description
repository
categories
platform support
installer
installed version
latest version
status
```

Actions:

```text
Install
Install & Play
Play
Update
Uninstall
Favorite
```

---

### Hour 70–72 — Library screen

Show only installed games.

Features:

```text
sort
search
play
update
uninstall
```

---

### Hour 72–74 — Favorites and Updates screens

Implement:

```text
Favorites
Updates
Update selected
Update all
```

---

### Hour 74–76 — Install/update progress UI

Installation must not freeze the interface.

Show:

```text
current step
progress where measurable
elapsed time
log summary
cancel availability where safe
```

---

### Hour 76–78 — TUI error dialogs and polish

Implement actionable errors.

Examples:

```text
Retry
View details
Back
Open repository
```

Review keyboard consistency.

**Phase Gate**

A user must be able to operate the main product entirely through the TUI.

---

# 11. Phase 7 — CLI, Configuration, Storage, and Diagnostics
## Hours 78–88

### Hour 78–80 — Complete CLI command tree

Implement:

```bash
rustarcade
rustarcade list
rustarcade search
rustarcade info
rustarcade install
rustarcade play
rustarcade update
rustarcade uninstall
rustarcade doctor
rustarcade catalog update
rustarcade catalog validate
rustarcade version
```

CLI and TUI must call the same services.

---

### Hour 80–82 — Configuration system

Implement:

```text
config.toml
defaults
loading
validation
safe persistence
```

Settings:

```text
theme
catalog refresh
update checks
confirmation behavior
experimental games
managed path overrides
```

---

### Hour 82–84 — Favorites and persistent application state

Implement atomic persistent storage for:

```text
favorites
recent list
preferences
```

Handle corrupted files gracefully.

---

### Hour 84–86 — Doctor command

Check:

```text
OS
architecture
terminal
filesystem permissions
Git
Cargo
Rustc
network
catalog validity
data directories
```

Output must clearly distinguish:

```text
OK
Warning
Failure
```

---

### Hour 86–88 — Logging system

Implement rotating or bounded logs where practical.

Categories:

```text
application
install
update
launch
catalog
```

Provide:

```bash
RUST_LOG=
rustarcade --debug
```

without leaking sensitive environment data.

---

# 12. Phase 8 — Updates, Uninstall, and Recovery
## Hours 88–98

### Hour 88–90 — Version resolution

Implement semantic comparison for:

```text
installed version
catalog version
GitHub latest release
Cargo crate version
```

Handle non-semver releases safely.

---

### Hour 90–92 — Update planning

Generate an update plan before modification.

Show:

```text
old version
new version
installer
source
destination
```

---

### Hour 92–94 — Update execution

Implement transactional updates:

```text
download/build new version
verify
swap installation
preserve old version until success
rollback on failure
```

---

### Hour 94–96 — Uninstall

Remove only RustArcade-owned files.

Do not delete arbitrary third-party save/config directories.

Clean the installation registry.

---

### Hour 96–98 — Recovery and broken-state repair

Detect:

```text
registry says installed but binary missing
partial directory
invalid metadata
interrupted update
```

Add repair suggestions and cleanup mechanisms.

---

# 13. Phase 9 — Security Hardening
## Hours 98–108

### Hour 98–100 — Command injection audit

Search the codebase for:

```text
sh -c
bash -c
cmd /C
PowerShell arbitrary strings
```

Remove unnecessary shell execution.

All known install actions must use structured arguments.

---

### Hour 100–102 — Filesystem safety audit

Validate:

```text
canonicalized paths
managed root boundaries
archive extraction
symlink behavior
temporary directories
atomic replacement
```

Add regression tests.

---

### Hour 102–104 — Network security

Require:

```text
HTTPS
reasonable timeout
redirect policy
content-length sanity where useful
user-agent
failure handling
```

Document GitHub trust assumptions.

---

### Hour 104–106 — Integrity verification

Implement SHA-256 verification where manifest/release metadata supports it.

On mismatch:

```text
abort
delete downloaded artifact
report security error
```

---

### Hour 106–108 — Security documentation and dependency audit

Create/complete:

```text
SECURITY.md
docs/THREAT-MODEL.md
```

Run available dependency/security checks such as:

```bash
cargo audit
```

if installed or installable without requiring user intervention.

Address actionable issues or document unavoidable upstream constraints.

---

# 14. Phase 10 — Initial Verified Game Catalog
## Hours 108–116

The goal is **at least 10 properly researched manifests**, not 50 unverified entries.

Suggested initial candidates:

```text
thomas-mauran/chess-tui
Strophox/tetro-tui
WanderHuang/game-2048-tui
Dalie-et/sudoku-tui
iShibi/snakeshell
scottnm/tetrust
indium114/termfarm
hiimsergey/mastermind-rs
ashxudey/terminal-poker
Cod-e-Codes/battleship-rs
```

---

### Hour 108–110 — Research installation methods

For every candidate, determine:

```text
active repository?
license?
supported platform?
Cargo crate?
GitHub releases?
binary name?
dependencies?
run command?
```

Do not invent manifest fields.

---

### Hour 110–112 — Create first five manifests

Create validated manifests and test installer planning.

Mark support status accurately:

```text
Verified
Community Tested
Experimental
```

---

### Hour 112–114 — Create next five manifests

Repeat the verification process.

Do not mark something Verified if it has not actually been validated sufficiently.

---

### Hour 114–116 — Catalog QA

Run complete catalog validation.

Check:

```text
duplicates
dead repositories
bad URLs
unsupported install strategies
bad commands
platform mismatches
```

Generate catalog documentation.

---

# 15. Phase 11 — Automated Testing, CI/CD, and Release Preparation
## Hours 116–123

### Hour 116–118 — Expand unit test coverage

Cover:

```text
manifest parser
validator
platform matching
asset matching
version comparison
safe paths
config
registry
favorites
history
```

---

### Hour 118–120 — Integration tests

Use local fixtures to avoid unreliable external network dependence.

Test:

```text
install fixture
verify fixture
launch fixture
exit code
update fixture
uninstall fixture
rollback fixture
```

---

### Hour 120–121 — CI workflow

Create:

```text
.github/workflows/ci.yml
```

Run on supported platforms where practical:

```text
Linux
macOS
Windows
```

Pipeline:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
rustarcade catalog validate
```

---

### Hour 121–122 — Release workflow

Create:

```text
.github/workflows/release.yml
```

Prepare binary artifacts for target platforms.

Do not require an actual public release to consider the repository commit-ready.

---

### Hour 122–123 — Catalog validation workflow

Create:

```text
.github/workflows/catalog-validation.yml
```

Ensure catalog changes are automatically checked in pull requests.

---

# 16. Phase 12 — Documentation, Final QA, and GitHub-Ready Cleanup
## Hours 123–128

### Hour 123–124 — Complete README

README should include:

```text
What RustArcade is
Screenshot/mock TUI example
Features
Installation
Usage
CLI
TUI controls
How game installation works
Security model
Catalog
Contribution
Development
License
```

---

### Hour 124–125 — Complete contributor documentation

Create:

```text
CONTRIBUTING.md
docs/ADDING-A-GAME.md
docs/DEVELOPMENT.md
```

Explain exactly how to add a manifest.

---

### Hour 125–126 — Repository hygiene

Inspect:

```bash
git status --short
git diff --check
```

Ensure `.gitignore` covers:

```text
target/
logs
temporary files
IDE state
local caches
secrets
environment files
test artifacts
```

Remove accidental binaries and generated junk.

---

### Hour 126–127 — Full release-candidate validation

Run the complete validation suite from a clean state:

```bash
cargo clean
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
cargo run -- doctor
cargo run -- catalog validate
```

Exercise representative CLI flows.

Exercise representative TUI flows.

Fix every reproducible failure before continuing.

---

### Hour 127–128 — Final Git and delivery audit

Review:

```bash
git status
git diff --stat
git diff
```

Perform secret scan where tooling is available.

Verify:

```text
no credentials
no API keys
no machine-specific absolute paths
no temporary test files
no broken documentation links under project control
no TODO blocking v1.0 behavior
```

Prepare a final implementation report containing:

```text
What was built
Architecture
Tests executed
Supported games
Known limitations
Security decisions
Commands for verification
Repository status
```

The repository must now be ready for:

```bash
git add .
git commit -m "feat: build initial RustArcade platform"
```

The agent should **not push to a remote unless explicitly authorized and authenticated**, but everything required for the commit must already be complete.

---

# 17. Autonomous Agent Checkpoints

The agent should maintain these quality gates internally.

## Checkpoint A — Hour 16

Expected:

```text
Clean Rust architecture
Core domain models
Platform detection
Storage paths
Passing tests
```

---

## Checkpoint B — Hour 30

Expected:

```text
Declarative catalog
Validated manifests
Search/list/info
Remote cache strategy
```

---

## Checkpoint C — Hour 48

Expected:

```text
Cargo installer
GitHub release installer
Registry
Rollback
```

---

## Checkpoint D — Hour 60

Expected:

```text
Safe game launcher
Terminal recovery
Play history
```

---

## Checkpoint E — Hour 78

Expected:

```text
Complete usable TUI
```

---

## Checkpoint F — Hour 98

Expected:

```text
CLI
Config
Doctor
Updates
Uninstall
Recovery
```

---

## Checkpoint G — Hour 108

Expected:

```text
Security hardening complete
```

---

## Checkpoint H — Hour 116

Expected:

```text
10+ validated catalog games
```

---

## Checkpoint I — Hour 123

Expected:

```text
Tests
CI
Release automation
```

---

## Checkpoint J — Hour 128

Expected:

```text
Complete GitHub-ready repository
```

---

# 18. Required Final Repository Structure

The exact structure may evolve, but the final result should resemble:

```text
RustArcade/
├── .github/
│   └── workflows/
│       ├── ci.yml
│       ├── release.yml
│       └── catalog-validation.yml
│
├── catalog/
│   ├── games/
│   └── schema/
│
├── docs/
│   ├── ADDING-A-GAME.md
│   ├── ARCHITECTURE.md
│   ├── DEVELOPMENT.md
│   ├── IMPLEMENTATION-CHECKLIST.md
│   ├── PRODUCT-SPEC.md
│   └── THREAT-MODEL.md
│
├── src/
│   ├── app/
│   ├── catalog/
│   ├── github/
│   ├── installer/
│   ├── launcher/
│   ├── platform/
│   ├── storage/
│   ├── tui/
│   ├── error.rs
│   └── main.rs
│
├── tests/
│   └── fixtures/
│
├── .gitignore
├── Cargo.lock
├── Cargo.toml
├── CHANGELOG.md
├── CONTRIBUTING.md
├── LICENSE
├── README.md
├── SECURITY.md
├── clippy.toml
└── rustfmt.toml
```

---

# 19. Required Quality Commands

Before completion, all applicable commands must succeed.

```bash
cargo fmt --check
```

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

```bash
cargo test --all-features
```

```bash
cargo build --release
```

```bash
cargo run -- doctor
```

```bash
cargo run -- catalog validate
```

If any command fails, the agent owns the diagnosis and repair.

---

# 20. Recommended Agent Working Loop

For every feature:

```text
Understand requirement
        ↓
Inspect existing architecture
        ↓
Research only if necessary
        ↓
Implement smallest clean design
        ↓
Add/update tests
        ↓
Run focused tests
        ↓
Run formatter
        ↓
Run Clippy
        ↓
Fix failures
        ↓
Update documentation
        ↓
Continue
```

Do not accumulate dozens of untested changes.

---

# 21. Failure Handling Policy

When a task fails, the agent should:

1. capture the exact error;
2. identify whether the problem is:
   - code;
   - dependency;
   - platform;
   - network;
   - upstream repository;
3. attempt a safe repair;
4. add a regression test when appropriate;
5. document genuine external limitations;
6. continue with other work only if the issue is not blocking the core product;
7. revisit all unresolved items before final completion.

A temporary upstream failure must not be confused with a RustArcade defect.

---

# 22. Scope Discipline

The agent should **not waste the 128-hour budget** on features outside the release target.

Defer until after the core product is complete:

```text
accounts
cloud saves
friends
centralized ratings
achievements
game streaming
web UI
mobile app
remote SSH hosting
advanced Linux sandboxing
multiplayer matchmaking
commercial store
```

Architecture may leave room for them, but they are not blockers for the initial complete product.

---

# 23. What “Complete” Means in This Schedule

“Complete” does **not** mean every possible future RustArcade feature exists.

It means the repository contains a polished initial production-quality RustArcade capable of delivering the intended core experience:

```text
Open RustArcade
      ↓
Browse terminal games
      ↓
Inspect game
      ↓
Install automatically
      ↓
Verify installation
      ↓
Launch game
      ↓
Play in terminal
      ↓
Exit game
      ↓
Return to RustArcade
      ↓
Update or uninstall later
```

with strong engineering foundations for future expansion.

---

# 24. Final AI Agent Instruction

The agent receiving this schedule should behave as the project owner and principal engineer for the full execution.

Its objective is not to merely create a prototype.

Its objective is to leave behind a repository that another developer can clone, understand, build, test, contribute to, and publish.

The user is **not part of the implementation loop**.

The agent must autonomously:

```text
plan
research
architect
implement
test
debug
refactor
secure
document
validate
clean
prepare
```

until the project is complete.

---

# 25. Final Deliverables

At completion, provide the user with a concise final report containing:

## Product

- RustArcade version/status;
- implemented features;
- supported platforms;
- supported installer methods;
- number of catalog games.

## Engineering

- architecture summary;
- important design decisions;
- test results;
- build results;
- security validation.

## Repository

- final file structure;
- Git status;
- commit-ready confirmation;
- suggested first commit message.

## Limitations

Only list genuine remaining limitations that:

- depend on external systems;
- are intentionally post-v1 scope;
- cannot reasonably be resolved inside the repository.

Do not list unfinished core work as a “future improvement.”

---

# 26. Suggested First Commit

When everything above is complete, the repository should be ready for:

```bash
git add .
git commit -m "feat: build RustArcade terminal game launcher"
```

A more detailed alternative:

```bash
git commit -m "feat: implement RustArcade game catalog, installer, launcher, TUI, and CLI"
```

---

# 27. Completion Statement

The final state expected after approximately **128 focused engineering hours** is:

> **RustArcade is fully implemented from an empty folder, tested, documented, security-reviewed, populated with an initial verified game catalog, equipped with CI/CD, and ready for its first GitHub commit without requiring implementation work from the user.**
