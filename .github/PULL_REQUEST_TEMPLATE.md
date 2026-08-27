## Summary

<!-- What does this change and why? Link related issues. -->

## Checklist

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-features` (offline; new behaviour has tests)
- [ ] `cargo run -- catalog validate` and `cargo run -- catalog index --check` (catalog changes)
- [ ] For new games: installed and launched through RustArcade in an isolated home
      (`RUSTARCADE_HOME=/tmp/ra cargo run -- install <id> --yes --play`) and `support_status` set honestly
- [ ] `CHANGELOG.md` updated under *Unreleased* for user-visible changes
- [ ] No shell commands, privilege escalation, or writes outside the managed directory introduced
