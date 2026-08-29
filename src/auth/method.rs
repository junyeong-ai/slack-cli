use serde::{Deserialize, Serialize};
use std::fmt;

use super::oauth::client::OAuthClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    Static,
    Pkce,
    ClientSecret,
}

impl AuthMethod {
    /// The browser flow a client is eligible for: a public client must prove
    /// possession with PKCE, a confidential one authenticates with its secret.
    pub fn for_client(client: &OAuthClient) -> Self {
        if client.is_public() {
            Self::Pkce
        } else {
            Self::ClientSecret
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Pkce => "pkce",
            Self::ClientSecret => "client-secret",
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
    use crate::auth::secret;

    #[test]
    fn public_clients_authorize_with_pkce() {
        let client = OAuthClient::public("id");
        assert_eq!(AuthMethod::for_client(&client), AuthMethod::Pkce);
    }

    #[test]
    fn confidential_clients_authorize_with_their_secret() {
        let client = OAuthClient::confidential("id", secret::new("shh"));
        assert_eq!(AuthMethod::for_client(&client), AuthMethod::ClientSecret);
    }

    #[test]
    fn serialized_names_are_stable() {
        for (method, name) in [
            (AuthMethod::Static, "\"static\""),
            (AuthMethod::Pkce, "\"pkce\""),
            (AuthMethod::ClientSecret, "\"client_secret\""),
        ] {
            assert_eq!(serde_json::to_string(&method).unwrap(), name);
        }
    }
}
