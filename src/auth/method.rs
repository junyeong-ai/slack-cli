use serde::{Deserialize, Serialize};
use std::fmt;

/// How a profile's tokens were obtained: pasted by the user, or issued by
/// Slack's browser flow. The browser flow is always PKCE — Slack requires it
/// for any redirect to a non-web URI, which is every address a CLI can listen
/// on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Static,
    Pkce,
}

impl AuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Pkce => "pkce",
        }
    }
}

impl fmt::Display for AuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_names_are_stable() {
        for (method, name) in [
            (AuthMethod::Static, "\"static\""),
            (AuthMethod::Pkce, "\"pkce\""),
        ] {
            assert_eq!(serde_json::to_string(&method).unwrap(), name);
        }
    }
}
