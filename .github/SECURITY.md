# Security Policy

## Supported versions

Only the latest minor release receives security fixes. Pin to a tagged release
for reproducible deployments and verify artifacts using the provenance and
sigstore signatures published with each release.

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
- A sigstore keyless signature (`*.sig`) and certificate (`*.pem`).
- A SLSA Level 3 provenance attestation (`slack-cli.intoto.jsonl`).

Verify a downloaded archive with `cosign`:

```sh
cosign verify-blob \
    --certificate slack-cli-v<version>-<target>.pem \
    --signature   slack-cli-v<version>-<target>.sig \
    --certificate-identity-regexp "^https://github.com/junyeong-ai/slack-cli/" \
    --certificate-oidc-issuer     "https://token.actions.githubusercontent.com" \
    slack-cli-v<version>-<target>.tar.gz
```

Verify the SLSA provenance with `slsa-verifier`:

```sh
slsa-verifier verify-artifact \
    --provenance-path slack-cli.intoto.jsonl \
    --source-uri      github.com/junyeong-ai/slack-cli \
    --source-tag      v<version> \
    slack-cli-v<version>-<target>.tar.gz
```
