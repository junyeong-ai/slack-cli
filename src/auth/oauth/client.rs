use serde::{Deserialize, Serialize};

/// The Slack app a profile authorized against.
///
/// Slack requires PKCE for any redirect to a non-web URI, and every address a
/// CLI can receive a callback on is one, so this is always a public client: it
/// proves possession with PKCE and its token exchanges carry no secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClient {
    pub id: String,
}

impl OAuthClient {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_serializes_as_its_id_alone() {
        let value = serde_json::to_value(OAuthClient::new("60503450.61416")).unwrap();
        assert_eq!(value["id"], "60503450.61416");
        assert_eq!(value.as_object().unwrap().len(), 1);
    }
}
