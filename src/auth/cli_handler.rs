use std::io::IsTerminal;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use secrecy::ExposeSecret;
use serde_json::{Value, json};

use crate::cli::AuthAction;
use crate::config::{AuthConfig, Config};
use crate::slack::{SlackClient, scopes};

use super::Authenticator;
use super::app_credential;
use super::credential::{Credential, TokenKind, TokenSet};
use super::login::{browser_login, static_login};
use super::method::AuthMethod;
use super::oauth::callback::DEFAULT_CALLBACK_PORT;
use super::oauth::client::OAuthClient;
use super::profile::Profile;
use super::secret::{self, Secret, mask as mask_secret};

pub async fn handle(
    action: AuthAction,
    profile: Option<String>,
    config: Config,
    authenticator: Arc<Authenticator>,
    json: bool,
) -> Result<()> {
    match action {
        AuthAction::Login {
            method,
            user_token,
            bot_token,
            app_token,
            client_id,
            port,
            no_browser,
        } => {
            let input = LoginInput {
                method: method.map(Into::into),
                profile: profile.and_then(non_blank),
                user_token: user_token.and_then(non_blank).map(secret::new),
                bot_token: bot_token.and_then(non_blank).map(secret::new),
                app_token: app_token.and_then(non_blank).map(secret::new),
                client_id: client_id.and_then(non_blank),
                port: port.unwrap_or(DEFAULT_CALLBACK_PORT),
                no_browser,
            };
            let slack = SlackClient::new(config.clone(), authenticator.clone())?;
            login(input, config, &slack, &authenticator, json).await
        }

        AuthAction::Logout { all, keep_remote } => {
            let slack = if keep_remote {
                None
            } else {
                Some(SlackClient::new(config, authenticator.clone())?)
            };
            logout(profile, all, slack.as_ref(), &authenticator, json).await
        }

        AuthAction::Status { verify } => {
            let slack = if verify {
                Some(SlackClient::new(config, authenticator.clone())?)
            } else {
                None
            };
            status(profile, slack.as_ref(), &authenticator, json).await
        }

        AuthAction::Profiles => list_profiles(&authenticator, json).await,

        AuthAction::Use { name } => set_active(name, &authenticator, json).await,

        AuthAction::Scopes => {
            print_scopes(&config.auth.exclude_scopes, json);
            Ok(())
        }
    }
}

struct LoginInput {
    method: Option<AuthMethod>,
    profile: Option<String>,
    user_token: Option<Secret>,
    bot_token: Option<Secret>,
    app_token: Option<Secret>,
    client_id: Option<String>,
    port: u16,
    no_browser: bool,
}

impl LoginInput {
    /// An app-level token on its own has no installation to create. It answers
    /// no `auth.test`, so there is no workspace it could name, and it is
    /// attached to a profile that already exists instead.
    fn attaches_only(&self) -> bool {
        self.app_token.is_some() && self.user_token.is_none() && self.bot_token.is_none()
    }

    /// The command line and the environment win; `config.toml` supplies the
    /// client id when they left it out.
    fn with_stored_app(mut self, auth: &AuthConfig) -> Self {
        self.client_id = self.client_id.or_else(|| auth.client_id.clone());
        self
    }
}

async fn login(
    input: LoginInput,
    config: Config,
    slack: &SlackClient,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    if let Some(token) = input.app_token.as_ref() {
        validate_app_token(token)?;
    }
    if input.attaches_only() {
        return attach_app_token(input, authenticator, json).await;
    }

    let method = decide_method(&input)?;
    let input = input.with_stored_app(&config.auth);

    let profile = match method {
        AuthMethod::Static => {
            let (user, bot) = collect_static_tokens(input.user_token, input.bot_token)?;
            static_login::run(user, bot, input.app_token.clone(), slack).await?
        }
        AuthMethod::Pkce => {
            let user_scopes = login_scopes(&config.auth.exclude_scopes)?;
            let request = browser_login::Request {
                client: build_client(input.client_id)?,
                api_base_url: config.connection.api_base_url.clone(),
                port: input.port,
                open_browser: should_open_browser(input.no_browser, std::io::stdin().is_terminal()),
                user_scopes,
            };
            browser_login::run(request).await?
        }
    };

    let auto_named = input.profile.is_none();
    let profile_name = input
        .profile
        .unwrap_or_else(|| slugify(&profile.workspace.team_name));

    if auto_named {
        let snapshot = authenticator.snapshot().await;
        if let Some(existing) = snapshot.profiles.get(&profile_name)
            && existing.workspace.team_id != profile.workspace.team_id
        {
            anyhow::bail!(
                "profile '{profile_name}' already maps to team '{}' ({}); \
                 re-run with --profile NAME to save '{}' ({}) under a distinct name",
                existing.workspace.team_name,
                existing.workspace.team_id,
                profile.workspace.team_name,
                profile.workspace.team_id,
            );
        }
    }

    authenticator
        .upsert_profile(&profile_name, profile.clone(), true)
        .await?;

    print_login_result(&profile_name, &profile, json);
    Ok(())
}

/// Attaches an app-level token to a profile that already exists.
///
/// The target is the same one every other command resolves — `--profile` or
/// the active one — so the token lands on the installation the daemon will
/// run as, and nowhere else.
async fn attach_app_token(
    input: LoginInput,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let token = input.app_token.expect("attaches_only implies an app token");

    let snapshot = authenticator.snapshot().await;
    let name = snapshot
        .resolve(input.profile.as_deref())
        .map(ToOwned::to_owned)
        .context(
            "an app-level token attaches to an existing profile, and none is configured. \
             Run `slack-cli auth login` first, then re-run this with --app-token",
        )?;

    authenticator
        .attach_token(&name, TokenKind::App, app_credential(token))
        .await?;

    if json {
        println!(
            "{}",
            json!({ "profile": name, "attached": TokenKind::App.as_str() })
        );
    } else {
        println!("\u{2713} Added app-level token to profile {name}");
    }
    Ok(())
}

/// An app-level token is checked by shape before it is stored. Slack has no
/// endpoint that validates one without opening a connection, and a `xoxb-`
/// pasted into `--app-token` is a plain slip worth catching here rather than
/// as a daemon that will not start.
fn validate_app_token(token: &Secret) -> Result<()> {
    if token.expose_secret().starts_with("xapp-") {
        return Ok(());
    }
    Err(anyhow!(
        "--app-token expects an app-level token starting with `xapp-`. \
         Create one under Basic Information \u{2192} App-Level Tokens with the \
         connections:write scope"
    ))
}

fn decide_method(input: &LoginInput) -> Result<AuthMethod> {
    if let Some(method) = input.method {
        return Ok(method);
    }
    if input.user_token.is_some() || input.bot_token.is_some() {
        return Ok(AuthMethod::Static);
    }
    if std::io::stdin().is_terminal() {
        Ok(AuthMethod::Pkce)
    } else {
        Err(anyhow!(
            "no authentication method selected. pass --method pkce|static, \
             provide --user-token/--bot-token, or run interactively"
        ))
    }
}

/// The user scopes a browser login asks for. An installation that has excluded
/// every one of them would open a consent screen granting nothing, so it is
/// refused here rather than in the browser.
fn login_scopes(excluded: &[String]) -> Result<Vec<String>> {
    let scopes = owned(scopes::requested(TokenKind::User, excluded));
    if scopes.is_empty() {
        anyhow::bail!(
            "auth.exclude_scopes leaves no scope to request, so the authorization would \
             grant nothing. Remove entries until at least one remains"
        );
    }
    Ok(scopes)
}

/// Opening a browser assumes someone is at this machine to see it. Without a
/// terminal there is no one — a script, a CI job, a test — and the URL printed
/// instead is what such a caller can act on.
const fn should_open_browser(no_browser: bool, interactive: bool) -> bool {
    !no_browser && interactive
}

fn build_client(client_id: Option<String>) -> Result<OAuthClient> {
    let client_id = client_id.context(
        "browser login requires a client id: pass --client-id, set SLACK_CLI_CLIENT_ID, \
         or put client_id under [auth] in config.toml",
    )?;
    Ok(OAuthClient::new(client_id))
}

fn collect_static_tokens(
    user: Option<Secret>,
    bot: Option<Secret>,
) -> Result<(Option<Secret>, Option<Secret>)> {
    let (user_token, bot_token) = if user.is_some() || bot.is_some() {
        (user, bot)
    } else if std::io::stdin().is_terminal() {
        let user = prompt_secret("User token (xoxp-..., recommended; leave blank to skip): ")?
            .map(secret::new);
        let bot = prompt_secret("Bot token (xoxb-..., optional; leave blank to skip): ")?
            .map(secret::new);
        (user, bot)
    } else {
        return Err(anyhow!(
            "static login requires --user-token or --bot-token in a non-interactive shell"
        ));
    };

    if user_token.is_none() && bot_token.is_none() {
        return Err(anyhow!(
            "at least one of --user-token or --bot-token is required"
        ));
    }

    Ok((user_token, bot_token))
}

/// Reads a token from the terminal without echoing it, so it never lands in
/// scrollback or a screen share.
fn prompt_secret(label: &str) -> Result<Option<String>> {
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let entered = rpassword::prompt_password(label)?;
    let trimmed = entered.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

fn non_blank(s: String) -> Option<String> {
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn owned(scopes: Vec<&'static str>) -> Vec<String> {
    scopes.into_iter().map(ToOwned::to_owned).collect()
}

fn slugify(input: &str) -> String {
    let joined: String = input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if joined.is_empty() {
        "workspace".to_string()
    } else {
        joined
    }
}

fn print_scopes(excluded: &[String], json: bool) {
    let user = scopes::requested(TokenKind::User, excluded);
    let bot = scopes::requested(TokenKind::Bot, excluded);
    // Not `requested`: an app-level token is minted in the app configuration
    // rather than granted by an authorization, so nothing about it is
    // excludable and the full requirement is always what to register.
    let app = scopes::required(TokenKind::App);
    if json {
        println!("{}", json!({ "user": user, "bot": bot, "app": app }));
    } else {
        println!("User Token Scopes:");
        println!("  {}", user.join(" "));
        println!("Bot Token Scopes:");
        println!("  {}", bot.join(" "));
        println!("App-Level Token Scopes (Basic Information \u{2192} App-Level Tokens):");
        println!("  {}", app.join(" "));
    }
}

fn print_login_result(profile_name: &str, profile: &Profile, json: bool) {
    if json {
        println!(
            "{}",
            json!({
                "profile": profile_name,
                "team_id": profile.workspace.team_id,
                "team_name": profile.workspace.team_name,
                "method": profile.method.as_str(),
                "tokens": token_kinds(&profile.tokens),
            })
        );
    } else {
        println!(
            "✓ Logged in to {} via {} (profile: {})",
            profile.label(),
            profile.method,
            profile_name,
        );
        for (kind, credential) in profile.tokens.iter() {
            println!("  {kind} token: {}", describe(credential));
        }
    }
}

fn token_kinds(tokens: &TokenSet) -> Vec<&'static str> {
    tokens.iter().map(|(kind, _)| kind.as_str()).collect()
}

fn describe(credential: &Credential) -> String {
    let masked = mask_secret(&credential.token);
    match credential.expires_at {
        Some(expiry) => {
            let renewal = if credential.refresh_token.is_some() {
                "renewable"
            } else {
                "not renewable"
            };
            format!(
                "{masked} (expires {}, {renewal})",
                expiry.format("%Y-%m-%dT%H:%M:%SZ")
            )
        }
        None => format!("{masked} (does not expire)"),
    }
}

fn credential_json(credential: &Credential) -> Value {
    json!({
        "token": mask_secret(&credential.token),
        "expires_at": credential.expires_at,
        "renewable": credential.refresh_token.is_some(),
        "scopes": credential.scopes,
    })
}

async fn logout(
    profile: Option<String>,
    all: bool,
    slack: Option<&SlackClient>,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let snapshot = authenticator.snapshot().await;

    let outcome = if all {
        if let Some(client) = slack {
            for (name, profile) in &snapshot.profiles {
                revoke_quietly(client, authenticator, name, &profile.tokens).await;
            }
        }
        authenticator.clear_all().await?;
        LogoutOutcome::All
    } else {
        let target = snapshot
            .resolve(profile.as_deref())
            .context("no active profile to log out from")?
            .to_string();

        if let Some(client) = slack
            && let Some(p) = snapshot.profiles.get(&target)
        {
            revoke_quietly(client, authenticator, &target, &p.tokens).await;
        }

        let found = authenticator.remove_profile(&target).await?.is_some();
        let was_active = snapshot.active_profile.as_deref() == Some(target.as_str());
        let new_active = authenticator.snapshot().await.active_profile;

        LogoutOutcome::Single {
            name: target,
            found,
            was_active,
            new_active,
        }
    };

    emit_logout_result(json, outcome);
    Ok(())
}

enum LogoutOutcome {
    All,
    Single {
        name: String,
        found: bool,
        was_active: bool,
        new_active: Option<String>,
    },
}

fn emit_logout_result(json: bool, outcome: LogoutOutcome) {
    if json {
        let payload = match &outcome {
            LogoutOutcome::All => json!({"scope": "all"}),
            LogoutOutcome::Single {
                name,
                found,
                was_active,
                new_active,
            } => json!({
                "scope": "single",
                "profile": name,
                "found": found,
                "was_active": was_active,
                "active_profile": new_active,
            }),
        };
        println!("{payload}");
        return;
    }

    match outcome {
        LogoutOutcome::All => println!("✓ Removed all profiles"),
        LogoutOutcome::Single {
            name,
            found,
            was_active,
            new_active,
        } => {
            if found {
                println!("✓ Removed profile {name}");
            } else {
                println!("Profile {name} was not found");
            }
            if was_active && new_active.is_none() {
                println!("  No active profile. Run: slack-cli auth use <NAME>");
            }
        }
    }
}

/// Revokes every token in a profile before it is dropped locally.
///
/// The token is renewed first when it is due: revoking with a token Slack has
/// already expired fails, which would leave the installation live at Slack
/// while the only credential that could revoke it is deleted here.
async fn revoke_quietly(
    slack: &SlackClient,
    authenticator: &Authenticator,
    profile: &str,
    tokens: &TokenSet,
) {
    for (kind, _) in tokens.iter() {
        // An app-level token is not an installation grant: `auth.revoke`
        // refuses it, and there is nothing at Slack to leave behind by
        // dropping it locally. Only the app's own configuration can retire it.
        if kind == TokenKind::App {
            continue;
        }
        match authenticator.token_for_profile(profile, kind).await {
            Ok(token) => {
                if let Err(err) = slack.auth.revoke(token.expose_secret()).await {
                    tracing::warn!("auth.revoke failed for the {kind} token: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("could not obtain a live {kind} token to revoke: {err}");
            }
        }
    }
}

async fn status(
    profile: Option<String>,
    slack: Option<&SlackClient>,
    authenticator: &Authenticator,
    json: bool,
) -> Result<()> {
    let snapshot = authenticator.snapshot().await;
    if snapshot.profiles.is_empty() {
        if json {
            println!("{}", json!({"profiles": []}));
        } else {
            println!("No profiles configured. Run: slack-cli auth login");
        }
        return Ok(());
    }

    let name = snapshot
        .resolve(profile.as_deref())
        .context("no active profile selected")?
        .to_string();
    let kind = snapshot
        .profiles
        .get(&name)
        .with_context(|| format!("profile {name} not found"))?
        .tokens
        .iter()
        .next()
        .map(|(kind, _)| kind);

    // Verification renews a token that is due, so it runs before the profile
    // is read for display: the report then describes the credential the
    // profile actually holds now, and matches what every other command sees.
    let verification = match (slack, kind) {
        (Some(client), Some(kind)) => match authenticator.token_for_profile(&name, kind).await {
            Ok(token) => Some(client.auth.test(token.expose_secret()).await),
            Err(err) => Some(Err(err.into())),
        },
        _ => None,
    };

    let snapshot = authenticator.snapshot().await;
    let profile = snapshot
        .profiles
        .get(&name)
        .with_context(|| format!("profile {name} not found"))?;

    if json {
        print_status_json(&name, &snapshot.active_profile, profile, verification);
    } else {
        print_status_text(&name, profile, verification);
    }

    Ok(())
}

type Verification = Option<Result<crate::slack::SlackAuthIdentity>>;

fn print_status_json(
    name: &str,
    active: &Option<String>,
    profile: &Profile,
    verification: Verification,
) {
    let mut payload = json!({
        "profile": name,
        "active": active.as_deref() == Some(name),
        "method": profile.method.as_str(),
        "workspace": profile.workspace,
        "client_id": profile.client.as_ref().map(|c| &c.id),
        "tokens": {
            "user": profile.tokens.user.as_ref().map(credential_json),
            "bot": profile.tokens.bot.as_ref().map(credential_json),
            "app": profile.tokens.app.as_ref().map(credential_json),
        },
        "authorized_at": profile.authorized_at,
    });
    match verification {
        Some(Ok(identity)) => {
            payload["verified"] = serde_json::to_value(&identity).unwrap_or(Value::Null);
        }
        Some(Err(err)) => {
            payload["verified"] = json!({"error": err.to_string()});
        }
        None => {}
    }
    println!("{payload}");
}

fn print_status_text(name: &str, profile: &Profile, verification: Verification) {
    println!("profile: {name} ({})", profile.method);
    println!(
        "  workspace: {} ({})",
        profile.workspace.team_name, profile.workspace.team_id
    );
    if let Some(client) = &profile.client {
        println!("  client_id: {}", client.id);
    }
    for (kind, credential) in profile.tokens.iter() {
        println!("  {kind} token: {}", describe(credential));
        if !credential.scopes.is_empty() {
            println!("    scopes: {}", credential.scopes.join(", "));
        }
    }
    if let Some(result) = verification {
        match result {
            Ok(identity) => println!("  verified : ok ({} / {})", identity.team, identity.user),
            Err(err) => println!("  verified : failed ({err})"),
        }
    }
}

async fn list_profiles(authenticator: &Authenticator, json: bool) -> Result<()> {
    let snapshot = authenticator.snapshot().await;
    if json {
        let payload: Vec<_> = snapshot
            .profiles
            .iter()
            .map(|(name, profile)| {
                json!({
                    "name": name,
                    "active": snapshot.active_profile.as_deref() == Some(name),
                    "method": profile.method.as_str(),
                    "workspace": profile.workspace,
                    "tokens": token_kinds(&profile.tokens),
                })
            })
            .collect();
        println!("{}", json!({"profiles": payload}));
    } else if snapshot.profiles.is_empty() {
        println!("No profiles configured. Run: slack-cli auth login");
    } else {
        for (name, profile) in &snapshot.profiles {
            let marker = if snapshot.active_profile.as_deref() == Some(name.as_str()) {
                "*"
            } else {
                " "
            };
            println!(
                "{} {:<20} {:<14} {}",
                marker,
                name,
                profile.method,
                profile.label()
            );
        }
    }
    Ok(())
}

async fn set_active(name: String, authenticator: &Authenticator, json: bool) -> Result<()> {
    let name = non_blank(name).context("profile name must not be blank")?;
    authenticator.set_active(&name).await?;
    if json {
        println!("{}", json!({"active": name}));
    } else {
        println!("✓ Active profile: {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(method: Option<AuthMethod>) -> LoginInput {
        LoginInput {
            method,
            profile: None,
            user_token: None,
            bot_token: None,
            app_token: None,
            client_id: Some("123.456".into()),
            port: DEFAULT_CALLBACK_PORT,
            no_browser: true,
        }
    }

    fn stored(client_id: Option<&str>) -> AuthConfig {
        AuthConfig {
            client_id: client_id.map(str::to_string),
            exclude_scopes: Vec::new(),
        }
    }

    #[test]
    fn slugify_lowercases_and_dashes_non_alnum() {
        assert_eq!(slugify("Acme Inc."), "acme-inc");
        assert_eq!(slugify("My Team!"), "my-team");
    }

    #[test]
    fn slugify_collapses_repeated_separators() {
        assert_eq!(slugify("foo---bar"), "foo-bar");
        assert_eq!(slugify("  foo  bar  "), "foo-bar");
    }

    #[test]
    fn slugify_falls_back_for_all_non_ascii_input() {
        assert_eq!(slugify("한국팀"), "workspace");
        assert_eq!(slugify(""), "workspace");
    }

    #[test]
    fn non_blank_trims_and_rejects_empty() {
        assert_eq!(non_blank("  ".into()), None);
        assert_eq!(non_blank("".into()), None);
        assert_eq!(non_blank("  abc  ".into()), Some("abc".to_string()));
    }

    #[test]
    fn pasted_tokens_select_the_static_method() {
        let mut given = input(None);
        given.user_token = Some(secret::new("xoxp-1"));
        assert_eq!(decide_method(&given).unwrap(), AuthMethod::Static);
    }

    #[test]
    fn an_explicit_method_always_wins() {
        let mut given = input(Some(AuthMethod::Static));
        given.user_token = None;
        assert_eq!(decide_method(&given).unwrap(), AuthMethod::Static);
    }

    #[test]
    fn the_command_line_outranks_the_stored_client_id() {
        let given = input(None).with_stored_app(&stored(Some("stored-id")));
        assert_eq!(given.client_id.as_deref(), Some("123.456"));
    }

    #[test]
    fn the_stored_client_id_fills_in_when_the_command_line_omits_it() {
        let mut given = input(None);
        given.client_id = None;
        let given = given.with_stored_app(&stored(Some("stored-id")));
        assert_eq!(given.client_id.as_deref(), Some("stored-id"));
    }

    #[test]
    fn a_login_that_would_request_nothing_is_refused() {
        let everything: Vec<String> = scopes::required(TokenKind::User)
            .into_iter()
            .map(str::to_string)
            .collect();
        let err = login_scopes(&everything).unwrap_err();
        assert!(err.to_string().contains("exclude_scopes"), "{err}");

        let kept = login_scopes(&everything[1..]).unwrap();
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn a_browser_opens_only_for_someone_at_a_terminal() {
        assert!(should_open_browser(false, true));
        assert!(!should_open_browser(true, true));
        assert!(!should_open_browser(false, false));
        assert!(!should_open_browser(true, false));
    }

    #[test]
    fn browser_login_requires_a_client_id() {
        let err = build_client(None).unwrap_err();
        assert!(err.to_string().contains("--client-id"));
    }

    /// `auth.revoke` takes an installation grant. Sending it an app-level
    /// token earns a refusal and a warning, and revokes nothing — the token
    /// only ever retires from the app's own configuration.
    #[test]
    fn logout_revokes_the_installation_tokens_and_not_the_app_one() {
        let mut tokens = TokenSet::default();
        tokens.set(
            TokenKind::User,
            Credential::permanent(secret::new("xoxp-1"), vec![]),
        );
        tokens.set(
            TokenKind::App,
            Credential::permanent(secret::new("xapp-1"), vec![]),
        );

        let revocable: Vec<TokenKind> = tokens
            .iter()
            .map(|(kind, _)| kind)
            .filter(|kind| *kind != TokenKind::App)
            .collect();
        assert_eq!(revocable, vec![TokenKind::User]);
    }

    #[test]
    fn an_app_token_alone_attaches_rather_than_logging_in() {
        let mut given = input(None);
        given.app_token = Some(secret::new("xapp-1-A01-1-abc"));
        assert!(given.attaches_only());

        given.user_token = Some(secret::new("xoxp-1"));
        assert!(!given.attaches_only());
    }

    #[test]
    fn an_app_token_is_refused_unless_it_looks_like_one() {
        assert!(validate_app_token(&secret::new("xapp-1-A01-1-abc")).is_ok());
        let err = validate_app_token(&secret::new("xoxb-not-an-app-token")).unwrap_err();
        assert!(err.to_string().contains("xapp-"), "{err}");
    }

    #[test]
    fn credentials_describe_their_lifetime() {
        let permanent = Credential::permanent(secret::new("xoxp-abcdefgh"), vec![]);
        assert!(describe(&permanent).contains("does not expire"));

        let rotating = Credential {
            token: secret::new("xoxe.xoxp-abcdefgh"),
            refresh_token: Some(secret::new("xoxe-refresh")),
            expires_at: Some(chrono::Utc::now()),
            scopes: vec![],
        };
        assert!(describe(&rotating).contains("renewable"));
    }
}
