## Summary

<!-- One to three sentences: what changes, and why. -->

## Changes

-

## Test plan

- [ ] `cargo nextest run --profile ci --all-features --workspace`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo deny check` (when touching dependencies)

## Checklist

- [ ] Tests cover the new behavior.
- [ ] Public CLI surface, configuration, or cache schema changes are documented.
- [ ] No new direct dependency added without justification (license, size, maintenance).
- [ ] Breaking changes follow the [Conventional Commits](https://www.conventionalcommits.org/) `!` marker so release-plz produces the correct semver bump.
