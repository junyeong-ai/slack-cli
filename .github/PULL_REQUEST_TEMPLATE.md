## Summary

<!-- One to three sentences: what changes, and why. -->

## Changes

-

## Test plan

- [ ] `cargo nextest run --profile ci --all-features --workspace --locked`
- [ ] `cargo clippy --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo deny --all-features --locked check` (when touching dependencies)

## Checklist

- [ ] Relevant checks pass; behavioral changes have focused regression coverage.
- [ ] Public CLI surface, configuration, or cache schema changes are documented.
- [ ] No new direct dependency added without justification (license, size, maintenance).
- [ ] Breaking changes follow the [Conventional Commits](https://www.conventionalcommits.org/) `!` marker.
