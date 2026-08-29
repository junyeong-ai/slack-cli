# update/ — `slack-cli self update`

Replaces the running executable with one published by a GitHub release.

## Layout

```
update/
├── mod.rs       The flow: resolve → confirm → download → verify → replace
├── release.rs   GitHub Releases API, asset lookup and download
├── install.rs   Checksum, signature, atomic replacement
└── target.rs    Which release asset this build updates itself from
```

## Why the release publishes a bare executable

Every target ships an archive *and* the executable itself. `install.sh`
downloads the archive; `self update` downloads the executable. That keeps the
CLI free of an archive reader — no gzip, tar or zip dependency — on the one
code path that overwrites the binary a user runs. The `latest` aliases mirror
only the archives, since `self update` always resolves a concrete release.

## Invariants

1. **Only a missing `cosign` may lower the bar.** The checksum is always
   verified and a mismatch aborts before anything is staged. `signature_policy`
   is the whole decision: with cosign installed the signature is mandatory, so a
   release that publishes no bundle is refused rather than accepted on a
   checksum its publisher also controls. The certificate is pinned to this
   repository's release workflow **for the exact tag being installed**, which
   refuses a signed binary lifted from another release. It cannot tell one
   asset of that tag from another: swapping in a different target's binary
   under this one's name verifies clean, which costs a wrong-architecture
   install and never foreign code, since every candidate blob was built by the
   same workflow run.
2. **Nothing is written outside a rename.** The download is staged beside the
   destination and renamed into place, so the destination is either the old
   binary or the new one. `Staged` removes the staging file on every path that
   does not complete the swap.
3. **The target is a compile-time fact.** `target::current()` is resolved from
   `cfg`, not probed: a musl build must replace itself with a musl build, and
   only the compiler knows which this is.
4. **Environment facts are inputs, not probes.** `api_base`, `binary` and
   `cosign` are `None` in production and detected there; tests pass them, so no
   test outcome depends on the machine running it. `tests/self_update.rs`
   asserts the update lands inside its temporary directory — without that guard
   a regression in the seam overwrites the test runner instead of failing.

## Platform difference

Unix renames over the running executable: the process holds its file by inode
and keeps running from the old one. Windows refuses that, so the running
executable is moved to `.old` first, and restored if the swap then fails — if
that restore also fails the error says where the old binary went. The displaced
file stays until a later update clears it, since this process still holds it
open.
