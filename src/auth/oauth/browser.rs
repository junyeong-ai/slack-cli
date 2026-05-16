use crate::auth::errors::OAuthError;

pub struct BrowserOpener {
    enabled: bool,
}

impl BrowserOpener {
    pub fn auto() -> Self {
        Self { enabled: true }
    }

    pub fn disabled() -> Self {
        Self { enabled: false }
    }

    pub fn open(&self, url: &str) -> Result<(), OAuthError> {
        if !self.enabled {
            return Err(OAuthError::BrowserFailed {
                url: url.to_string(),
            });
        }
        open::that(url).map_err(|_| OAuthError::BrowserFailed {
            url: url.to_string(),
        })
    }
}
