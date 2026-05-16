use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngExt;
use sha2::{Digest, Sha256};

const VERIFIER_LEN: usize = 64;
const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";

pub struct PkceVerifier(String);

pub struct PkceChallenge(String);

impl PkceVerifier {
    pub fn new() -> Self {
        let mut rng = rand::rng();
        let bytes: String = (0..VERIFIER_LEN)
            .map(|_| {
                let idx = rng.random_range(0..UNRESERVED.len());
                UNRESERVED[idx] as char
            })
            .collect();
        Self(bytes)
    }

    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn challenge(&self) -> PkceChallenge {
        let digest = Sha256::digest(self.0.as_bytes());
        PkceChallenge(URL_SAFE_NO_PAD.encode(digest))
    }
}

impl Default for PkceVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl PkceChallenge {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_example_vector() {
        // RFC 7636 Appendix B.
        let verifier = PkceVerifier::from_raw("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        assert_eq!(
            verifier.challenge().as_str(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn new_verifier_has_required_length() {
        let v = PkceVerifier::new();
        assert_eq!(v.as_str().len(), VERIFIER_LEN);
    }

    #[test]
    fn new_verifier_uses_only_unreserved_characters() {
        let v = PkceVerifier::new();
        for c in v.as_str().chars() {
            assert!(
                UNRESERVED.contains(&(c as u8)),
                "character {c:?} not in unreserved set"
            );
        }
    }
}
