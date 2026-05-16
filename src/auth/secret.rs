use secrecy::ExposeSecret;
use serde::{Deserialize, Deserializer, Serializer};

pub use secrecy::SecretString as Secret;

pub fn new(value: impl Into<String>) -> Secret {
    Secret::new(value.into().into_boxed_str())
}

pub fn mask(secret: &Secret) -> String {
    let value = secret.expose_secret();
    if value.len() <= 8 {
        return "*".repeat(value.len());
    }
    match (value.get(..4), value.get(value.len() - 4..)) {
        (Some(prefix), Some(suffix)) => format!("{prefix}...{suffix}"),
        _ => "***".to_string(),
    }
}

/// Serde adapter for `Option<Secret>`: round-trips through `Option<String>`
/// while keeping the raw value behind `Secret` in memory.
pub mod option {
    use super::*;

    pub fn serialize<S: Serializer>(value: &Option<Secret>, ser: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(secret) => ser.serialize_some(secret.expose_secret()),
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Secret>, D::Error> {
        let opt = Option::<String>::deserialize(de)?;
        Ok(opt.map(new))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_short_token_with_stars() {
        assert_eq!(mask(&new("abc")), "***");
        assert_eq!(mask(&new("12345678")), "********");
    }

    #[test]
    fn masks_long_token_with_prefix_and_suffix() {
        assert_eq!(mask(&new("xoxp-123456789")), "xoxp...6789");
    }

    #[test]
    fn empty_token_yields_empty_mask() {
        assert_eq!(mask(&new("")), "");
    }
}
