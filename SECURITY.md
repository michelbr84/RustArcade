# Security Policy

RustArcade installs and runs **third-party software**. Security is therefore a core feature
of the launcher, but it has limits that you should understand before using it.

## What RustArcade protects against

| Threat | Protection |
|---|---|
| Arbitrary command execution from the catalog | Manifests are declarative. Only three typed installers exist (`cargo`, `github-release`, `git-cargo-build`). Unknown fields such as `install = "curl … \| sh"` are rejected by the parser. No installer ever runs a shell; every tool is spawned with an explicit argument vector and a closed stdin. |
| Shell / argument injection | Crate names, git references, paths and executable names are validated against strict character sets; references may not start with `-`; git receives `--` before user-controlled values. |
| Path traversal | Every manifest path is checked (no absolute paths, no `..`, no drive prefixes, bounded depth). All writes happen inside the RustArcade data directory and are re-checked against it before renames and deletions. |
| Archive traversal (zip slip) | Entries with absolute paths, `..` components or prefixes are rejected before extraction. Symbolic and hard links inside archives are never created. Extraction is bounded (2 GiB, 20 000 entries) and archive types are confirmed by magic bytes. |
| Tampered downloads | All downloads use HTTPS with an HTTPS-only redirect policy. SHA-256 digests are verified from, in order of preference: the manifest, the GitHub API `digest` field, or a checksum file published with the release. A mismatch aborts the install, deletes the artifact and reports a security error (exit code 3). |
| Partial or corrupt installations | Installs are transactional: files are built in a staging directory, verified, then swapped in; the previous version survives until the registry is updated; failures roll back and leave a log. Interrupted swaps are repaired on the next start. |
| Writes outside managed directories | Uninstall removes only paths recorded in the registry, each verified to live under the data directory. Game save files and configuration outside RustArcade are never touched. |
| Privilege escalation | RustArcade never requires or requests administrator privileges. It never runs `sudo`, package managers, or upstream install scripts. |
| Secret leakage | Environment variables are never logged. `GITHUB_TOKEN` (optional) is only sent to the GitHub API over HTTPS. Manifests cannot set `PATH`, `LD_*` or `DYLD_*` variables for launched games. |

## What RustArcade does **not** do

* It does **not** sandbox games. A launched game runs with your user's privileges and full
  access to your files, network and terminal, exactly as if you had installed it yourself.
* It does **not** audit upstream source code or binaries. The catalog records that a project
  was installable and launchable at verification time, not that it is trustworthy.
* It does **not** verify code signatures. Checksums protect against corruption and tampering
  in transit, and against the asset changing after the digest was recorded; they do not prove
  who built the binary.
* It does **not** protect against a compromised upstream repository or GitHub account. Updates
  fetch whatever the project publishes.

Before installing a game RustArcade always shows the source repository, installation method,
version, destination and required tools. Read that plan.

## Reporting a vulnerability

Please report security issues **privately** through GitHub's
[private vulnerability reporting](https://github.com/michelbr84/RustArcade/security/advisories/new)
rather than a public issue. Include the RustArcade version (`rustarcade version`), your
platform, and steps to reproduce.

You can expect an acknowledgement within a few days. Fixes are released as patch versions and
described in `CHANGELOG.md`.

Catalog problems (a manifest pointing at a malicious or compromised project) should be reported
the same way; the entry will be removed or marked `broken` in the remote catalog, which every
RustArcade installation refreshes automatically.

## Supported versions

Only the latest release receives security fixes.

See also: `docs/THREAT-MODEL.md`.
