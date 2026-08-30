# Security Policy

## Supported versions

Only the latest minor release receives security fixes. Pin to a tagged release
for reproducible deployments and verify artifacts using the provenance and
sigstore signatures published with each release.

## Credential storage

`slack-cli` writes every credential it holds — user and bot access tokens and
their refresh tokens — to `auth.json` in the platform config directory, with mode
`0600` and its parent directory tightened to `0700` on Unix. The file is
rewritten atomically and guarded by an advisory lock so concurrent invocations
cannot interleave writes. Permissions are re-tightened on every read, and a
loosening is reported on stderr.

Tokens are held in memory behind `secrecy::SecretString`, which zeroizes on
drop and masks `Debug` output. They are never written to `config.toml`, never
logged, and are masked in every command's output.

Never commit `auth.json`, and treat it as equivalent to the tokens themselves.
`slack-cli auth logout` revokes each token in the profile with Slack before
removing it, unless `--keep-remote` is passed. Revocation is best effort: a
token that can no longer be presented — expired, with its refresh token
already spent — is reported and skipped rather than blocking the local
removal.

## Updating in place

`slack-cli self update` downloads the executable a release publishes, verifies
its SHA-256 against the published checksum, and — when `cosign` is installed —
verifies the sigstore bundle. Its identity pin is stricter than the one above:
because the updater always knows the version it is installing, it anchors the
tag, refusing a validly-signed binary taken from a different release. With
`cosign` installed a release that publishes no signature is refused; without it
the download rests on its checksum alone and the command says so. The new file is staged beside the
destination and renamed into place, so an interrupted update leaves the existing
binary untouched.

## Reporting a vulnerability

Report vulnerabilities **privately** through GitHub Security Advisories:

<https://github.com/junyeong-ai/slack-cli/security/advisories/new>

Please include:

- A clear description of the issue and its impact.
- A minimal reproduction (commands, configuration, or test case).
- The affected version(s) and the platform you observed the issue on.
- Any suggested mitigation, if known.

We will acknowledge your report within **5 business days** and aim to ship a
fix or mitigation within **30 days** of triage. Coordinated disclosure is
preferred — please refrain from public disclosure until a release is
available.

## Verifying release artifacts

Every release publishes:

- A `.tar.gz` or `.zip` archive per target.
- A SHA-256 checksum (`*.sha256`).
- A sigstore keyless signature bundle (`*.bundle`).
- A SLSA Level 3 provenance attestation (`slack-cli.intoto.jsonl`).

Verify a downloaded archive with `cosign`. The identity is pinned to the
tag-triggered release workflow, so a signature produced by any other workflow
or branch fails:

```sh
cosign verify-blob \
    --bundle slack-cli-v<version>-<target>.tar.gz.bundle \
    --certificate-identity-regexp \
        "^https://github.com/junyeong-ai/slack-cli/\.github/workflows/release\.yml@refs/tags/" \
    --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
    slack-cli-v<version>-<target>.tar.gz
```

`scripts/install.sh` runs exactly this check when `cosign` is on `PATH`.

Verify the SLSA provenance with `slsa-verifier`:

```sh
slsa-verifier verify-artifact \
    --provenance-path slack-cli.intoto.jsonl \
    --source-uri      github.com/junyeong-ai/slack-cli \
    --source-tag      v<version> \
    slack-cli-v<version>-<target>.tar.gz
```
