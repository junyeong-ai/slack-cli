//! The README is this project's user-facing reference, so the scope lists it
//! publishes must be the ones `auth login` actually requests. Both are derived
//! from the same API registry here, and drift fails the build rather than
//! sending users to configure a Slack app that cannot serve the CLI.

use slack_cli::slack::api_config::TokenKind;
use slack_cli::slack::scopes;

const KOREAN: &str = include_str!("../README.md");
const ENGLISH: &str = include_str!("../README.en.md");

fn documented(language: &str, readme: &str, marker: &str) -> Vec<String> {
    let open = format!("<!-- scopes:{marker} -->");
    let close = format!("<!-- /scopes:{marker} -->");
    let start = readme
        .find(&open)
        .unwrap_or_else(|| panic!("{language} is missing the `{open}` marker"))
        + open.len();
    let end = readme
        .find(&close)
        .unwrap_or_else(|| panic!("{language} is missing the `{close}` marker"));

    let mut scopes: Vec<String> = readme[start..end]
        .replace("```", " ")
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect();
    scopes.sort();
    scopes
}

fn required(kind: TokenKind) -> Vec<String> {
    scopes::required(kind)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn every_readme_publishes_the_scopes_the_cli_requests() {
    for (language, readme) in [("README.md", KOREAN), ("README.en.md", ENGLISH)] {
        for (marker, kind) in [("user", TokenKind::User), ("bot", TokenKind::Bot)] {
            assert_eq!(
                documented(language, readme, marker),
                required(kind),
                "{language} documents the wrong {marker} scopes; \
                 run `slack-cli auth scopes` and update the block"
            );
        }
    }
}
