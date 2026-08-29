use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::auth::secret::{self, Secret};

/// The Slack app credentials a profile authorized against.
///
/// A client without a secret is a *public* client: Slack requires PKCE for it,
/// routes its loopback redirect as a desktop redirect — which cannot carry bot
/// scopes and always issues rotating tokens — and accepts its token exchanges
/// with no secret at all. A client with a secret is *confidential*: it
/// authenticates with HTTP Basic, may request bot scopes, and receives
/// rotating tokens only when the app enables token rotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClient {
    pub id: String,

    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "secret::option"
    )]
    pub secret: Option<Secret>,
}

impl OAuthClient {
    pub fn public(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            secret: None,
        }
    }

    pub fn confidential(id: impl Into<String>, secret: Secret) -> Self {
        Self {
            id: id.into(),
            secret: Some(secret),
        }
    }

    pub fn is_public(&self) -> bool {
        self.secret.is_none()
    }

    /// The `Authorization: Basic` value Slack prefers over form-encoded
    /// credentials, or `None` for a public client, which sends none.
    pub fn basic_auth(&self) -> Option<String> {
        let secret = self.secret.as_ref()?;
        let raw = format!("{}:{}", self.id, secret.expose_secret());
        Some(format!("Basic {}", STANDARD.encode(raw)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_client_carries_no_credentials() {
        let client = OAuthClient::public("60503450.61416");
        assert!(client.is_public());
        assert!(client.basic_auth().is_none());
    }

    #[test]
    fn confidential_client_encodes_rfc7617_basic_credentials() {
        let client = OAuthClient::confidential("id", secret::new("shh"));
        assert!(!client.is_public());
        assert_eq!(client.basic_auth().unwrap(), "Basic aWQ6c2ho");
    }

    #[test]
    fn secret_is_omitted_from_serialized_public_clients() {
        let value = serde_json::to_value(OAuthClient::public("id")).unwrap();
        assert_eq!(value["id"], "id");
        assert!(value.get("secret").is_none());
    }

    #[test]
    fn secret_round_trips_through_serde() {
        let client = OAuthClient::confidential("id", secret::new("shh"));
        let encoded = serde_json::to_string(&client).unwrap();
        let decoded: OAuthClient = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.secret.unwrap().expose_secret(), "shh");
    }
}
