# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.20.3] - 2026-10-24

### Changed

- Under `--json`, `self update` needs `--yes`, as it does off a terminal, instead of asking at a terminal. A `--json` caller is a program, and the question was the one stderr line that was not JSON.

### Fixed

- A command that logs a warning, such as a rate-limit retry, while its stderr is a closed pipe exits with its usual status instead of aborting. 0.20.1 covered only the error message.
- When `SSL_CERT_FILE` or `SSL_CERT_DIR` is set but no certificate can be read from it, the switch to the bundled Mozilla roots is a warning instead of a debug message.
- An invalid `RUST_LOG` is ignored as a whole and reported as a warning, a JSON record under `--json`, instead of in a plain-text line that kept its valid directives.
- The event loop in the bundled skill and the READMEs answers a mention posted with a file, a thread reply also sent to the channel, and a `/me` message. It answered only messages without a subtype and acknowledged the rest unanswered.
- `curl … | bash`, the documented way to run `install.sh`, stopped at `BASH_SOURCE[0]: unbound variable` before installing anything. It has failed since 0.14.0; running the script from a file was unaffected.

## [0.20.2] - 2026-10-24

### Fixed

- On a Linux system without CA certificates, such as a minimal container, every command that loads credentials failed with `failed to create OAuth client: builder error`, including ones that make no request. Such a system now trusts the Mozilla roots built into the binary; `SSL_CERT_FILE` and `SSL_CERT_DIR` still choose the system's.
- Socket Mode trusts the operating system's certificate store like every other connection, instead of only the Mozilla roots built into the binary, so a root installed on the system also applies to the daemon.

## [0.20.1] - 2026-10-24

### Changed

- In `--json` mode every stderr line is a JSON object: log records with `level` and `fields`, `auth login`'s `{"authorize_url", "browser_opened"}`, and on a runtime failure the error envelope as the last line. A rate-limit or token-renewal warning used to arrive as colored text ahead of the envelope.
- Progress messages such as `Fetching users...` are whole lines, so a warning logged during the fetch no longer lands in the middle of one.
- The bundled `slack-workspace` skill pre-approves only commands that change nothing in Slack. Sending, editing, deleting, scheduling, uploading and other writes now go through Claude Code's permission prompt.

### Fixed

- Logs are colored only when stderr is a terminal, and `NO_COLOR` turns color off. Pipes, files and service-manager logs no longer receive escape codes.
- A command whose stderr is a closed pipe exits with its usual status instead of aborting.
- `self update` asks for confirmation on stderr, so stdout carries only the outcome, one JSON document under `--json`.

## [0.20.0] - 2026-10-24

### Added

- `schedule`, `scheduled` and `unschedule` hand a message to Slack to post later, list what this token scheduled, and cancel it. A time must name an instant (Unix seconds or RFC 3339 with an offset), and `--metadata` is refused because Slack never posts a scheduled message that carries it. Uses the existing `chat:write`.
- `upload <channel> <path>` streams a file through Slack's external upload and shares it in a channel or thread (`--title`, `--comment`, `--thread`, `--alt-text`). Needs `files:write`.
- `dm <user>[,<user>…]` prints the channel ID of the DM with one user or the group DM with up to eight, opening it with the token `send` uses. Needs `im:write` and `mpim:write`.
- Messages carry a lean `files` list (`id, name, title, mimetype, size, mode, permalink`) by default, so a message that only shares a file no longer reads as empty text.
- `thread` takes `--cursor`.

### Changed

- **Breaking:** `thread --json` returns the `{messages, next_cursor}` envelope `messages` uses instead of a bare array. Each call reads one page instead of looping internally, which under the commercial distribution's one-request-a-minute limit took minutes. Slack repeats the parent on every page; later pages now carry replies only.
- **Breaking:** `--expand` and `config.toml [output].*_fields` accept only the documented field names. An unknown flag value exits `2` with the valid names, and an unknown config entry fails config loading instead of printing nothing; `config path` and `config edit` still run.
- **Breaking:** `cache refresh --json` reports `{"ok": true, "unreadable": [{type, scope}]}` instead of `{"status": "ok"}`, the shape the other write acknowledgements use, plus the conversation types the token may not list.
- `--json` writes one line when stdout is piped and indents on a terminal.
- New scopes: `files:write`, `im:write` and `mpim:write`. Existing tokens keep working, and only `upload` and `dm` need them. Register them on the Slack app before the next `auth login`, or list the ones the app or workspace does not allow in `[auth].exclude_scopes`.
- Dependencies, GitHub Actions and CI tools are updated to their latest releases. Workflows are audited with zizmor; release builds no longer restore caches, the release is published with the runner's `gh`, and the Codecov upload that never succeeded is removed.

### Fixed

- A token without one conversation type's scope, such as `groups:read`, still resolves channels of the other types: the directory lists each configured type separately and reports the refused ones.
- A channel name that does not resolve says whether the directory was fetched during the command and which conversation types it lacks, instead of always suggesting a refresh. A name that belongs to an archived channel is answered with that channel's ID.
- Event recovery reads a subscribed thread's missed replies by `oldest` instead of Slack's cursor, which skips the first reply of every later page once `oldest` is set; long threads lost one reply per page boundary. Slack's repeated parent no longer counts toward the recovery limit.
- Writing output no longer panics. A reader that exits early, such as `| head`, ends the command with status `0`, and output that cannot be serialized fails the command with an error.

## [0.19.8] - 2026-10-17

### Fixed

- Require the Slack Web API's top-level boolean `ok` field before acknowledging success. Reject malformed HTTP-success responses instead of reporting successful writes or token revocation.
- Return invalid-response errors without replaying requests or including response content, while preserving Slack's normal success, warning and API-error responses.

## [0.19.7] - 2026-10-17

### Fixed

- Pin installer signature verification to the exact release tag selected for download, matching self-update verification and rejecting signatures from another release.

### Documentation

- Align manual signature verification, metadata retry guidance and stored-event consumption instructions with the implementation.
- Scope PKCE and credential-resolution guidance to the CLI's authentication paths.

## [0.19.6] - 2026-10-17

### Fixed

- Reject event retention periods that cannot be represented as signed seconds before opening the event log, preventing overflow from shortening retention or deleting retained events.
- Validate recovery duration and date ranges with checked arithmetic, returning configuration errors instead of panicking on out-of-range backfill periods.

## [0.19.5] - 2026-10-17

### Fixed

- Stage self-updates in exclusively created temporary files and copy through their open handles. Preserve pre-existing staging files and link targets instead of overwriting or deleting them.
- Use the temporary-file lifecycle for cleanup and publication, retaining Windows replacement and rollback behavior.

## [0.19.4] - 2026-10-17

### Fixed

- Recheck recorded token expiry after failed renewal and state-lock waits, refusing credentials that expired during those waits while preserving the renewal error.

### Documentation

- Align README, skill and developer guidance with rotating app-level tokens: stored login accepts non-rotating `xapp-` tokens; `SLACK_APP_TOKEN` may supply externally managed rotating access tokens and requires a daemon restart on replacement.
- Clarify logout revocation boundaries and distinguish missing expiry metadata from a guarantee of token validity.

## [0.19.3] - 2026-10-17

### Fixed

- Commit event deduplication keys and channel/thread positions atomically after delivery. A failed write leaves the entire completion state retryable, preventing stale recovery positions and repeated backfill after a restart.

## [0.19.2] - 2026-10-17

### Fixed

- Reject non-finite and out-of-range numeric date filters instead of silently changing search bounds or sending invalid message-history timestamps. Preserve message timestamp precision.
- Calculate cache refresh thresholds without truncating fractional hours or overflowing large TTL values, preventing premature stale-cache notices.

## [0.19.1] - 2026-10-17

### Documentation

- Align README and skill guidance with environment-token precedence, stored-profile verification results, static token renewal and error handling.
- Use explicit PKCE selection in browser-login examples, acknowledge events after handling them, and refresh caches without deleting unrelated profiles or assuming default paths.
- Correct developer guidance for profile collision protection, credential readiness and complete directory pagination; clarify environment credential storage in the security policy.

## [0.19.0] - 2026-10-17

### Fixed

- Preserve the first retained event ID and sequence during push redelivery after a crash, including Socket Mode messages recovered through history. Keep current event content, source and matching rules.
- Read the retained identity in the insertion transaction so concurrent pruning cannot remove it between those operations.

### Changed

- `EventStore::append` returns `AppendOutcome { id, seq, inserted }` instead of `Option<i64>`. Storage schemas are unchanged; existing retained rows remain compatible.

## [0.18.0] - 2026-10-17

### Fixed

- Exclude all verified human and bot actors from event rules by default, preventing own-bot reply loops while retaining primary-user mentions, reactions and explicit own-message opt-in.
- Resolve user-ID DM targets with the actual operation credential and use that same credential for the request. Never reuse another sender's cached DM, create a DM during lookup or fall back to a different sender.

### Changed

- Daemon startup validates both installation identities and rejects mixed workspaces. Add `AuthError::WorkspaceMismatch`, `SlackAuthClient::identities` and `RuleEngine::with_own_identity`.
- User-ID targets query accessible existing DMs directly; they no longer require an IM cache refresh. Direct conversation IDs retain their behavior and permissions.

## [0.17.1] - 2026-10-17

### Fixed

- Bound stdout delivery to one process-owned writer so an unread pipe cannot accumulate workers or prevent daemon shutdown. Retire stdout after a timeout, cancellation or write failure to preserve line boundaries.
- Serialize uninstall with installation using the same persistent kernel lock, preventing mixed binary and skill state.
- Honor `--json` for config and cache paths with the same `{"path": "..."}` output as event paths, including config repair commands.

## [0.17.0] - 2026-10-17

### Fixed

- Register event consumers at their first pull so another consumer's acknowledgement cannot prune their pending events.
- Check automatic profile-name collisions inside the authentication store transaction, preserving other workspaces during concurrent logins.
- Preserve existing skill paths before every replacement, including incomplete directories, files and broken symlinks.
- Accept empty user and channel snapshots, remove stale rows and search entries, and use sync metadata to distinguish empty caches from uninitialized ones.

### Changed

- Add `AuthError::ProfileNameConflict` for protected profile saves. CLI collision errors retain exit code 1.

## [0.16.0] - 2026-10-17

### Fixed

- Require unique exact channel names for operations instead of selecting partial search matches.
- Validate all static user/bot credentials and reject mixed workspaces. Preserve concurrent logins during logout and revoke only the selected authorization.
- Isolate event stores by endpoint, profile, workspace and user. Re-login and credential rotation for the same owner preserve the store; verified environment bindings support offline reads.
- Apply immutable per-rule subscription boundaries to live and recovered replies. Apply the configured recovery age limit to threads as well as channels.
- Serialize installer runs with a kernel lock across binary and skill installation.
- Correct skill discovery examples, retention guarantees and update documentation.

### Changed

- Event storage uses owner-bound directories. Stop older daemons and finish or export pending events with the old binary before upgrading. Older directories are preserved without importing unverified ownership; consumers and subscriptions start fresh.
- The shell installer requires Perl with core Fcntl on macOS and Linux.

## [0.15.0] - 2026-10-17

### Fixed

- Remove stale FTS tokens when users and channels are replaced, renamed or deleted. Cache schema 4 rebuilds existing indexes.
- Isolate caches by authorization and API endpoint, including environment credentials. Stored token rotation keeps its cache; a new login starts a fresh one. Legacy shared caches are not imported.
- Retain durable channel and thread recovery checkpoints until their queued events have been processed. Interrupted and partial recovery survives live traffic and restarts; stale reconnect completions cannot clear newer checkpoints.
- Preserve the complete previous skill directory before replacement, stop on preservation errors, and restore the prior installation when a swap fails.

### Changed

- Replace the library configuration helper `Config::db_path` with `Config::cache_dir`; the CLI selects an authorization-specific database below that directory.
- Stop the daemon after persistent storage or commit failures so unfinished recovery checkpoints remain available on restart.

## [0.14.1] - 2026-10-17

### Fixed

- Build ARM64 Linux musl releases with the native cross linker instead of the legacy compiler wrapper, whose shell tracing is reported as a linker warning by Rust 1.99.

## [0.14.0] - 2026-10-17

### Changed

- Remove the cache `with_lock` helper and lease constants; snapshot writes coordinate through SQLite transactions. The cache schema advances to version 3 and rebuilds on first use.
- Update the pinned toolchain to Rust 1.99.1, refresh direct and transitive dependencies, and remove redundant development dependencies and unused features.
- Run SQLite work on bounded blocking workers and separate event command handling from pipeline processing.
- Apply rate limiting to every request attempt and share HTTP 429 cooldowns across calls to the same method.

### Fixed

- Request daemon shutdown through its profile state database on every platform instead of matching process names and signalling PIDs. Existing daemons must be stopped through their service manager and restarted with this binary.
- Match installed skills to the binary version, replace installed binaries atomically and stop failed downloads without switching to a source build.
- Correct event acknowledgement examples, metadata deduplication guidance and configuration defaults.
- Store the skill version in supported frontmatter metadata and read legacy installed versions during upgrades.

## [0.13.1] - 2026-08-31

### Fixed

- `daemon status` reported a daemon that had stopped or crashed as still running for up to 90 seconds. `daemon stop` decides liveness from the daemon lock, for the reason it documents — a daemon killed a moment ago leaves a fresh heartbeat behind — but `status` decided from the heartbeat's age alone, so it answered the same question a different way, and answered it wrong in exactly the window a user looks in right after stopping one. Both now ask the lock. The heartbeat keeps the job it can do: a daemon that still holds the lock but has stopped heartbeating is wedged rather than gone, and says so instead of reading as healthy

### Changed

- `taiki-e/install-action` and `softprops/action-gh-release` pinned forward in the CI and release workflows; the latter no longer turns a malformed GitHub API error into a secondary failure while publishing assets

## [0.13.0] - 2026-08-31

### Added

- **A Socket Mode daemon: `watch`, `daemon run|status|stop`, `events pull|ack|stats|prune|path`.** Slack caps `conversations.history` and `conversations.replies` at one request a minute and 15 messages for a non-Marketplace app, so polling cannot follow a workspace — a channel every minute is the whole budget. Events are pushed on a separate axis (30,000 per workspace per hour), and RTM, the other way of receiving them, accepts no new apps and ends in November 2026. `watch` streams matching events to stdout and stores nothing; `daemon run` is the long-lived form, meant to run under launchd or systemd
- **`config.toml [events]`.** `mode` decides how long an event is kept — `stream` keeps nothing, `spool` keeps it until a consumer acknowledges it, `archive` keeps it for `retention_days` — and `store_body` decides how much of it, so an installation that may not keep other people's message text can still record which threads moved. Cursors, thread subscriptions and deduplication keys live in a separate always-durable database that holds no message content, which is what lets `stream` recover from a disconnect while keeping nothing
- **Declarative rules.** `mentions_me`, `keywords`, `from_users`, a `channels` allowlist, and `subscribe_emoji` — reacting to a message subscribes its thread and every later reply matches, so an emoji becomes a subscribe button. Rules are validated when the config loads, including the combinations that would silently never fire: a blank keyword that matches everything, a text predicate on a rule that only sees reactions, a subscribing rule that never sees the removal that would unsubscribe it
- **`TokenKind::App` and `auth login --app-token` / `SLACK_APP_TOKEN`.** The `xapp-` token that opens a Socket Mode connection is issued by no OAuth grant, never rotates, and authorizes nothing in a workspace, so it is a third axis rather than a third flavour of installation credential. `TokenPolicy` keeps it disjoint from the user and bot axes in both directions, which is what stops `connections:write` reaching an authorization request — Slack refuses a grant that mixes the two. On its own the flag attaches to an existing profile rather than creating one, because there is no `auth.test` an app-level token can answer
- `events.backfill_max_age_hours` clamps where a recovery read *starts* rather than deciding whether a channel is recovered at all. A daemon down over a weekend has cursors older than the window; excluding those channels would have lost everything inside the window too, in silence. A clamped read that completes leaves the cursor at the horizon, so a quiet channel is not re-read on every reconnect — the cost is one `conversations.history` request per recoverable channel on the first reconnect after a long absence, paced in the background and never on the live path
- Gap recovery. Socket Mode replays nothing across a disconnect, so a reconnect reads back what it missed — `conversations.history` for channels and `conversations.replies` for subscribed threads, each from a stored cursor, bounded by a channel count, an age and a page limit. Recovered events are replayed oldest-first and are never the ones an overflow discards, because Slack will not send them again

### Fixed

- An `exec` sink whose handler exits without reading its input was reported as a failed delivery. A handler that acts on the fact of an event rather than its contents is entitled to do that; the write then lands on a pipe it has already closed, and the broken pipe was being counted rather than its exit status. Caught by CI on macOS, where the race falls the other way than on Linux
- `scripts/uninstall.sh` offered to remove the configuration directory without saying that it now also holds the event store. The cache is refetchable and a login replaces the auth store, but Socket Mode does not replay: an event collected and not yet read with `events pull` is there and nowhere else, so the prompt names it
- `.env.example` documented `DATA_PATH`, which nothing has ever read. The cache and event-store locations are `[cache] data_path` and `[events] data_path` in `config.toml`

## [0.12.0] - 2026-08-30

### Added

- **`config.toml [auth].exclude_scopes`.** Slack approves a scope set as a whole, so an app that may not register one of the scopes this CLI derives cannot be installed at all — which left such a workspace with no browser login. Exclusions subtract from the derived set rather than replacing it, so a method added to the registry still reaches every installation, and each entry is checked against that set: a name no method needs is refused when the config loads. `auth scopes` prints the effective set, so what you register on the Slack app is what the login requests
- A command refused for a scope now says which one: `missing_scope. bookmarks.list needs bookmarks:read`. Slack reports only that a scope is missing, while the registry already knows what the method declares

### Fixed

- **A browser opened for callers with no one to see it.** `auth login` opened one whenever `--no-browser` was absent, so a script, a CI job or a test could put an authorization window on a desktop. It opens only when the command is run from a terminal; anywhere else the URL is printed, which such a caller can act on
- **`--config` naming a file that is not there loaded defaults and exited zero,** discarding the argument. A config that could not be stat'd — behind a trailing slash, or an unsearchable parent — vanished the same way. Only a default location that has never been created falls back to defaults now
- **`config edit` was refused by the very config it repairs.** It and `config path` run before the config loads, so any rejected value can still be fixed. `config edit` refuses a path that is not a file, which the load used to catch
- `--port 0` asks for an ephemeral port, which no app can register as a Redirect URL; it produced `http://127.0.0.1:0/callback` and a refusal from Slack after the browser had opened. It is rejected as the usage error it is

## [0.11.0] - 2026-08-30

### Removed

- **`auth login --method client-secret`, `--client-secret`, `SLACK_CLI_CLIENT_SECRET` and `config.toml [auth].client_secret`.** Slack refuses a loopback redirect that omits PKCE — *Must use PKCE to redirect to a non-web URI* — and refuses bot scopes on one outright — *Bot scopes are not allowed when redirecting to a non-web URI*. Every address a CLI can receive a callback on is such a URI, so the confidential flow could never reach a consent screen and the bot token it advertised was never obtainable through a browser. Both messages come from Slack's own authorization endpoint. A bot token is registered by pasting it into `--method static`, which is where its scope list now lives. A `client_secret` left in `config.toml` is refused rather than ignored

### Fixed

- **Every browser login failed at the consent screen.** The requested user scopes included `metadata.message:read`, which Slack supports on bot and legacy tokens only. A scope set is granted as a whole, so the one unsupported entry failed the whole authorization with `Invalid permissions requested` — for every PKCE login since the flow was introduced in 0.8.0. The scope is now requested for bot tokens alone
- **A rejected `config.toml` printed its own contents.** `toml` reports a parse failure by quoting the offending line, and a config the CLI refuses is often one still holding a credential a newer schema no longer accepts — a `user_token` dropped before 0.5.0, a `client_secret` dropped in this release. The refusal repeats on every invocation, so the value reached the terminal and anything capturing it each time. The error now reports the file, line and column instead
- `config edit` no longer creates `config.toml` with mode `0600`; 0.10.0 added that for the client secret the file no longer holds

## [0.10.0] - 2026-08-30

### Added

- `config.toml` carries the OAuth app under `[auth]` — `client_id`, and optionally `client_secret` — so a browser login no longer needs a flag or an environment variable on every run. The environment was the only non-flag source before, and a value placed there is inherited by every process the CLI spawns, while a `.env` sits in whatever working directory the command ran from, usually a git repository. `config.toml` is a `0600` file inside a `0700` directory and belongs to no repository. Flags and the environment still outrank it, so CI is unaffected

### Fixed

- `client_secret` is read from the config but never written back, so `config show --json` — which serializes the whole config — cannot print it; the human view masks it. `config edit` now creates a missing `config.toml` already restricted to `0600` rather than tightening it afterwards, closing the window in which a fresh file is readable by anyone

## [0.9.0] - 2026-08-30

### Added

- `self update` replaces the running binary with a published release, with `--check` to report whether one exists, `--version` to name a release, and `--yes` to skip the prompt. Releases now publish the executable itself beside each archive, so the CLI needs no gzip, tar or zip reader on the one path that overwrites the binary a user runs
- The download is always checked against the published SHA-256, and against its sigstore signature when `cosign` is installed. The certificate is pinned to this repository's release workflow for the exact tag being installed, which refuses a validly-signed binary lifted from another release and re-uploaded under this one's asset names. With `cosign` installed a release that publishes no signature is refused outright — the checksum is produced by whoever published the binary, so on its own it says nothing about origin
- The replacement is staged in the destination directory and renamed into place, so an interrupted update leaves the existing binary untouched. Windows moves the running executable aside first and restores it if the swap fails

The executable asset exists from this release onward, so `--version` naming an earlier release is refused with a message saying so; use `scripts/install.sh` for those.

## [0.8.0] - 2026-08-30

### Added

- `auth login --method client-secret`: confidential-client OAuth using an app's client id and secret, authenticating with HTTP Basic. Slack routes the loopback redirect of a non-PKCE app as a server redirect, so this flow can request bot scopes and issue a `xoxb-` token in the same pass — something the PKCE flow structurally cannot do, because Slack treats a PKCE app's loopback as a desktop redirect and refuses bot scopes there
- `auth scopes` prints the OAuth scopes to register on the Slack app, derived from the API methods the CLI calls. `tests/documented_scopes.rs` holds both READMEs to that same set, and `tests/readme_parity.rs` fails the build when a command, flag, environment variable or exit code is documented in one language and not the other
- `assistant.search.info` via `search --capabilities`, reporting whether the workspace can rank semantically, plus the `--modifiers` (e.g. `has:pin from:@alice`) and `--include-deleted-users` parameters on `search`
- Windows support: config, auth store and cache now resolve through the platform base-directory convention (`%APPDATA%` on Windows, `$XDG_CONFIG_HOME` or `~/.config` elsewhere — byte-identical to the previous behaviour on Unix). CI now runs the test suite on `windows-latest`, which the release workflow has been shipping binaries for untested

### Fixed

- **Rotating tokens are renewed instead of expiring.** Slack issues a 12-hour access token and a refresh token for every PKCE installation, and for confidential apps with token rotation enabled. Neither field was parsed, so a PKCE profile stopped working 12 hours after login and needed a fresh `auth login`. Credentials now carry their expiry and refresh token, and are exchanged before use once they enter the renewal window
- **Concurrent invocations can no longer lose each other's writes.** Slack revokes a refresh token as soon as it is used, and the auth store was a read-modify-write with no locking. Every mutation now runs under an advisory lock on a sibling `auth.json.lock`, re-reading from disk inside the lock; `AuthStore::write` requires the lock guard, so an unlocked write does not compile
- **`search` requested the wrong scope.** `auth login` asked for the legacy `search:read`, which covers `search.messages` and not the Real-time Search API, so `slack-cli search` failed with `missing_scope` on every PKCE profile. Scopes are now declared per method in the API registry and unioned from there, which is what removes the hand-maintained list that had drifted
- `auth status --verify` and `auth logout` renewed nothing and used the stored token directly: verification reported a false failure on a profile every other command used successfully, and revocation silently failed against Slack while the local credential was deleted. Both now renew first
- A renewal that cannot complete — no client recorded, the exchange refused, the network down — no longer fails a command whose token is still valid; it warns, proceeds on the credential in hand, and retries next invocation. The failure is only fatal once the token is genuinely past its expiry
- `RUST_LOG` was documented but ignored: the log filter was built from a literal string. It is now read from the environment, and `.env` is loaded before arguments are parsed, so `SLACK_PROFILE`, `SLACK_CLI_CLIENT_ID` and `SLACK_CLI_CLIENT_SECRET` are honoured from a `.env` file too
- Token and client-secret prompts no longer echo to the terminal
- Bot and user scopes are recorded separately per credential rather than collapsed into one list
- `SECURITY.md` documented `cosign verify-blob` against `.sig` and `.pem` artifacts that releases do not publish; the command now matches the signature bundle the release workflow actually produces

### Changed

- **Breaking:** the auth store moves to schema 2. Existing stores are upgraded in place on first open. Profiles created by the previous PKCE flow hold a token that has already expired with no refresh token to renew it, so those need one `auth login`
- **Breaking:** `auth status --json` reports each token as `{token, expires_at, renewable, scopes}` rather than a masked string, and scopes move from the profile onto the credential
- **Breaking:** minimum supported Rust is 1.98.0
- Every pinned GitHub Action moved to its current release, and a scheduled workflow now opens a `cargo update` pull request for the transitive versions Dependabot does not track

## [0.7.1] - 2026-07-11

### Fixed

- `messages` / `thread` no longer fail an entire page when Slack emits a message's `channel` field as a bare id string instead of an `{id, name}` object — both wire forms deserialize, output always serializes the object form. Busy channels hit this deterministically while paging with `--cursor`

## [0.7.0] - 2026-07-11

### Added

- `--markdown-text` on `send` and `update`: standard-Markdown message body rendered by Slack itself (`chat.postMessage`/`chat.update` `markdown_text`, max 12,000 chars) — no mrkdwn hand-translation needed. Mutually exclusive with `--text`/`--blocks`, enforced at both the clap and payload-validation boundaries
- Typed `SlackApiError` (`Api`, `RateLimitExhausted`, `Http`, `Transport`) replaces string-only Slack core errors; the CLI boundary classifies failures by downcast into differentiated exit codes — `0` ok, `1` generic, `2` usage (clap), `3` auth, `4` rate-limited — and `--json` mode prints an `{"error": {code, message}}` envelope to stderr for runtime failures, with Slack's own error string preserved verbatim as `code`
- `scripts/install.sh` verifies release signatures with cosign (sigstore bundle pinned to the tag-triggered release workflow identity) when cosign is installed, compares SHA-256 digests directly, and auto-detects glibc vs musl on Linux

### Changed

- **BREAKING**: `slack-cli messages --json` now emits a `{messages, next_cursor}` envelope instead of a bare array, exposing the `conversations.history` cursor so channel history is actually pageable via `--cursor`; `next_cursor` is `null` on the last page. `thread --json` keeps its bare-array shape (it paginates internally)
- Rust toolchain 1.95.0 → 1.97.0 (`rust-toolchain.toml` is the single pin; the duplicate `.tool-versions` that overrode it via `RUSTUP_TOOLCHAIN` is removed) and rusqlite 0.39 → 0.40 / r2d2_sqlite 0.34 → 0.35, picking up rusqlite's tainted-SAVEPOINT SQL-injection fix

### Fixed

- Cache schema migration actually runs: the stored `schema_version` is compared on open and any mismatch rebuilds every cache object inside a single `BEGIN IMMEDIATE` transaction, so concurrent processes serialize instead of interleaving DROP/CREATE; a non-integer stored version rebuilds instead of failing the open
- First-open `database is locked` races: the WAL switch happens once at pool creation (not per connection) with a bounded busy retry, since SQLite journal-mode transitions bypass the busy handler
- Release "latest" alias assets get regenerated `.sha256` files; the copied checksums previously referenced the versioned filenames and could never verify

### Documentation

- README (KO + EN), the `slack-workspace` skill, and module guides document the pagination envelope, `--markdown-text`, the exit-code / error-envelope contract (usage errors exit `2` with clap diagnostics by design), and the install verification chain; the skill's `allowed-tools` narrows from blanket `Bash` to `Bash(slack-cli *)` + `Bash(jq *)`

## [0.6.0] - 2026-05-19

### Added

- Introduce `MessagePayload`, the unified content surface for `chat.postMessage` and `chat.update` (text, blocks, attachments, metadata). The CLI exposes it via `-t/--text`, `-b/--blocks`, `-a/--attachments`, `-m/--metadata`; each JSON-source flag accepts `-` (stdin, max one per call), `@path.json`, or inline JSON, with array-vs-object shape validated before any HTTP call
- `slack-cli permalink <channel> <ts>` and `messages.permalink(channel, ts)` wrap `chat.getPermalink`
- `SlackMessage` exposes a typed `metadata` field; `conversations.history` and `conversations.replies` always request `include_all_metadata=true` so idempotency markers round-trip without an extra flag
- `SlackAuthIdentity` surfaces `url`, `bot_id`, `enterprise_id`, `enterprise_name`, and `is_enterprise_install` from `auth.test`; PKCE user-scope set gains `metadata.message:read`
- `[output] messages_fields` config key with a lean AI-first default (`ts`, `user`, `bot_id`, `username`, `text`, `thread_ts`, `reply_count`, `subtype`, `metadata`); rich fields are opt-in via `--expand` on `messages` and `thread`, both of which now also accept `--exclude-bots` for symmetry
- `<channel>` arguments accept `U…` / `W…` user IDs and auto-resolve to that user's cached DM channel (requires `im` in `cache.channel_types`)

### Changed

- **BREAKING**: `slack-cli send <channel> <text>` is now `slack-cli send <channel> -t <text>` (at least one of `text` / `blocks` / `attachments` required). `slack-cli update` mirrors the same shape minus `--thread`
- **BREAKING**: `slack-cli messages --json` projects through `messages_fields`; previously-implicit `blocks` / `attachments` / `reactions` / `permalink` fields require `--expand`
- **BREAKING**: `SlackMessageClient::{send, update}` library signatures take a `MessagePayload`
- **BREAKING**: `config.toml` rejects unknown keys (`deny_unknown_fields`); stale entries (`user_token`, `bot_token`, `connection.max_idle_per_host`, `connection.pool_idle_timeout_seconds`) now surface as explicit parse errors instead of being silently ignored
- **BREAKING**: HTTP connection-pool tuning (`max_idle_per_host`, `pool_idle_timeout_seconds`) is no longer a `[connection]` knob — the previous defaults are internal constants inside the Slack core

### Fixed

- `SlackChannel.name` and `MessageChannel.name` are now `Option<String>` — DM channels from `conversations.list?types=im` arrive without a `name` field, which previously crashed `cache refresh` with `missing field 'name'`. DMs round-trip through the cache cleanly, and `SlackChannel.user` exposes the DM peer

### Documentation

- Align README (KO + EN), the `slack-workspace` skill, and the per-module `CLAUDE.md` files with the new send / update / permalink surface, the channel-identifier table covering `U…` user IDs, the JSON source forms, and the lean `messages_fields` default

## [0.5.0] - 2026-05-16

### Added

- Introduce a multi-method authentication subsystem (`slack-cli auth login`) supporting `static` (paste an existing `xoxp-` / `xoxb-` token) and `pkce` (OAuth Authorization Code + PKCE with an embedded `client_id`); tokens persist to `${XDG_CONFIG_HOME:-~/.config}/slack-cli/auth.json` (mode `0600`, atomic write) keyed by named profiles
- `slack-cli auth {login, logout, status, profiles, use}` subcommand group; global `--profile` (env: `SLACK_PROFILE`) selects the active profile per invocation and is accepted at any position
- `SLACK_USER_TOKEN` / `SLACK_BOT_TOKEN` env vars bypass the store entirely for CI / headless use

### Changed

- **BREAKING**: Remove `bot_token` / `user_token` keys from `config.toml`; tokens now live in `auth.json` only
- **BREAKING**: Remove `--token` / `--user-token` global CLI flags
- **BREAKING**: Remove `slack-cli config init`; use `slack-cli auth login` instead

### Documentation

- Restructure root `CLAUDE.md` for progressive disclosure with a new `src/auth/CLAUDE.md` covering the auth subsystem
- Replace the `config init` flow with the `auth login` workflow in both `README.md` and `README.en.md`
- Align the `slack-workspace` Claude Code skill with the new auth flow and document the actual JSON response shape per command

### Fixed

- Correct skill JSON shape claims for `emoji`, `reactions`, `users`, and `channels` so generated `jq` queries match the real output envelope

## [0.4.0] - 2026-05-14

### Added

- Expand RTS option coverage with `--channel`, `--before`, `--after`, `--include-archived`, and `--no-semantic` flags; `highlight` and `include_message_blocks` auto-toggle by output mode

### Changed

- **BREAKING**: Align all client methods with verb-only naming (`messages.send`, `messages.history`, `messages.replies`, `users.list`, `channels.list`, `channels.members`, etc.); remove dead `pub` plumbing (`post_message`, `get_thread_replies`, `*_streaming` variants)
- **BREAKING**: Drop the `assistant.search.info` capabilities path and rename `SlackSearchClient::search` to `context`; remove `SearchCapabilities`
- Annotate `context()` failure with the `search:read.*` scope requirement so auth errors surface an actionable message

### Documentation

- Restructure `CLAUDE.md` with progressive disclosure: slim root file plus nested `src/slack/CLAUDE.md` and `src/cache/CLAUDE.md`; align `README` and skill manifest with the actual CLI surface

### Fixed

- Paginate `search.context` to the user-requested total instead of capping at a single 20-result page; raise `--limit` ceiling to 100
