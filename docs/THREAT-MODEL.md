# Threat Model

This document describes the assets RustArcade protects, the actors it trusts, and how each
identified threat is mitigated in the code. Read it together with `SECURITY.md`.

## Assets

1. The user's machine: files, credentials, shell environment, other processes.
2. The RustArcade data directory (`~/.local/share/rustarcade` on Linux): installed games,
   registry, favorites and play history.
3. The integrity of installed games (a game must be what its project published).
4. The user's terminal state (never left in raw mode or the alternate screen).

## Trust boundaries

```
┌─────────────────────────────────────────────────────────────────┐
│ Trusted                                                         │
│   RustArcade binary · built-in catalog (compiled in)             │
├─────────────────────────────────────────────────────────────────┤
│ Verified before use                                             │
│   remote catalog (HTTPS + per-file SHA-256 + strict validation)  │
│   release assets (HTTPS + SHA-256 when available)                │
│   GitHub / crates.io API responses (HTTPS, schema-parsed)        │
├─────────────────────────────────────────────────────────────────┤
│ Untrusted                                                       │
│   contents of downloaded archives · cloned repositories          │
│   crates fetched by cargo · the games themselves at runtime      │
└─────────────────────────────────────────────────────────────────┘
```

RustArcade trusts its own code and the catalog compiled into it. Everything fetched from the
network is verified for **integrity** and **shape** but the *behaviour* of a game is never
audited: once launched, a game is ordinary user-level software.

## Actors

| Actor | Capability | Handled by |
|---|---|---|
| Malicious catalog contributor | Submits a manifest that tries to run commands, write outside the install directory, or fetch from an insecure URL | Declarative schema, strict validation in `src/catalog/validate.rs`, CI validation on every PR, `deny_unknown_fields` |
| Compromised remote catalog host / MITM | Serves a modified catalog | HTTPS only, `index.json` with per-file SHA-256, validation of the whole set before an atomic swap, refusal of an index older than the cached one |
| Malicious or compromised release asset | Archive with traversal entries, symlinks, huge files, wrong content type | `src/install/archive.rs`: path checks before extraction, links skipped, size/entry caps, magic-byte verification, executable format check |
| Network attacker | Redirects to plain HTTP, serves tampered bytes | HTTPS-only client with an HTTPS-only redirect policy (`src/net/http.rs`), streaming SHA-256, download size cap |
| Compromised upstream project | Publishes a malicious version | Out of scope for prevention; mitigated by showing the source before install, by remote catalog updates that can mark an entry `broken`, and by never granting elevated privileges |
| Local attacker with write access to the data directory | Replaces a binary or edits the registry | Out of scope (same user account). The registry only ever points inside the data directory and the launcher refuses executables outside it |

## Threats and mitigations

### T1 — Command injection through manifests
*Vector:* a manifest field ends up in a shell.
*Mitigation:* there is no shell. `Command::new(tool).args(vec)` is the only process API used
(`src/install/process.rs`). Field grammars: crate names `[A-Za-z0-9_-]`, git references
`[A-Za-z0-9._/-]` without leading `-` or `..`, executables `[A-Za-z0-9._-]` with no separators.
The `--` separator precedes URLs and paths passed to git.

### T2 — Path traversal and writes outside managed directories
*Mitigation:* `paths::safe_relative` rejects absolute paths, `..`, drive prefixes, backslashes,
control characters and deep nesting. `AppPaths::ensure_managed` is checked before staging is
created, before the registry records an executable, and before every uninstall deletion.

### T3 — Archive traversal, symlink and bomb attacks
*Mitigation:* tar entries are validated with `check_entry_path`; `unpack_in` returning `false`
is treated as an error rather than a skip. Zip entries must have an `enclosed_name`. Link
entries are never materialised. Limits: 2 GiB extracted, 20 000 entries, 200 MB download.

### T4 — Tampered or corrupted downloads
*Mitigation:* SHA-256 computed while streaming and compared with the manifest digest, the
GitHub API digest, or a release checksum file. Mismatch → artifact deleted → `SecurityError`
→ nothing registered. Installs without any checksum source are allowed but labelled
"not verified" in the plan and registry; `install.require_checksum = true` forbids them.

### T5 — Partial installation corruption
*Mitigation:* staging → verify (file exists, ELF/Mach-O/PE magic, executable bit) → rename
swap → registry write (atomic temp-file + rename) → cleanup. Any failure before the registry
write removes staging; failure of the registry write reverts the swap.
`sweep_stale` at startup removes `staging-*` and restores `previous-*` when `current` is missing.

### T6 — Terminal corruption
*Mitigation:* the launcher suspends the TUI (raw mode off, alternate screen left) before
spawning a game and resumes afterwards through a guard that also runs on early return or panic.
Ctrl+C during a game only reaches the game; RustArcade ignores it while a child runs.

### T7 — Secret leakage
*Mitigation:* logs contain command lines and tool output, never the environment. Manifests
cannot set `PATH`, `HOME`, `LD_*`, `DYLD_*`. `GITHUB_TOKEN` is only attached to requests to the
configured GitHub API base.

## Residual risks

* Games are not sandboxed. A malicious game can do anything your user can do.
* `cargo install` builds untrusted crates whose build scripts run with your privileges.
  This is inherent to the Rust ecosystem; RustArcade shows the source before building.
* Release assets without checksums rely on TLS alone.
* The remote catalog is authenticated by TLS and integrity-checked, not signed. Signing is a
  planned improvement.
