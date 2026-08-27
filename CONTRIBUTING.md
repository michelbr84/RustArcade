# Contributing to RustArcade

Thanks for helping build the terminal arcade. This guide covers the two most common
contributions: adding a game to the catalog and changing the launcher itself.

## Ground rules

- Be kind and constructive in issues and reviews; the [Code of Conduct](CODE_OF_CONDUCT.md) applies everywhere.
- Use the issue forms: *Bug report*, *Game request* and *Feature request*. Questions go to
  [Discussions](https://github.com/michelbr84/RustArcade/discussions).
- Keep the security model intact: manifests stay declarative, installers stay typed, no shell
  execution, no privilege escalation. If a change needs any of these, open an issue first.
- Every change must keep the quality gates green:

  ```bash
  cargo fmt --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-features
  cargo run -- catalog validate
  cargo run -- catalog index --check
  ```

## Adding a game

1. Research the project (see `docs/ADDING-A-GAME.md` for the checklist): is it a terminal game,
   which installers apply, what executable does it produce, what license, which platforms.
2. Create `catalog/games/<id>.toml`. The `id` must be lowercase letters, digits and dashes and
   equal the file name.
3. Validate and regenerate the index:

   ```bash
   cargo run -- catalog validate catalog/games/<id>.toml
   cargo run -- catalog index
   ```

4. Install and launch it through RustArcade in an isolated home:

   ```bash
   RUSTARCADE_HOME=/tmp/ra-test cargo run -- install <id> --yes --play
   ```

5. Set `support_status` honestly (`verified` only if you ran step 4 successfully, with
   `verified_on = "YYYY-MM-DD"`), then open a pull request. CI validates the manifest.

## Changing the code

- Read `docs/ARCHITECTURE.md` for the module map. The CLI and TUI are thin layers over
  `src/services.rs`; new behaviour belongs in the services or below.
- Add tests next to the code for pure logic and integration tests under `tests/` for behaviour
  that touches the filesystem or processes. Tests must not need the network: use the fixture
  binary and `httpmock` like the existing suites.
- Keep error messages actionable: a title, the detail, possible causes, and a log path when
  one exists (see `src/error.rs`).
- Document user-visible changes in `CHANGELOG.md` under *Unreleased*.

## Commit style

Conventional commits are appreciated (`feat:`, `fix:`, `catalog:`, `docs:`, `ci:`), one logical
change per commit.

## Reporting security issues

Do not open public issues for vulnerabilities — see `SECURITY.md`.
