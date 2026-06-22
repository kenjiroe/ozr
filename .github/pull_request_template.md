## Summary

What changed and why?

## Test plan

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] `./scripts/audit-secrets.sh` (before release-related changes)

## Security impact

- [ ] No secrets committed
- [ ] Approval / policy behavior unchanged, or documented above
