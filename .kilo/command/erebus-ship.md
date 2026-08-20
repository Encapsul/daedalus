# /erebus-ship — Release checklist for erebus

Uses gstack's `/ship` skill to automate the release flow.

## Steps (from /ship)
1. Run clippy + fmt + tests: `cargo clippy -- -D warnings && cargo test --workspace`
2. Build release: `cargo build --release --bin erebus`
3. Bump version in Cargo.toml + CHANGELOG.md
4. Commit + tag: `git commit -S -m "feat: ..." && git tag v0.x.0`
5. `git push && git push --tags`

## Security checks (from /cso)
- No secrets in metadata
- Ed25519 keys have Ed25519 bit set (CVE-2023-48022)
- Stub has SAFETY comments on all unsafe blocks
