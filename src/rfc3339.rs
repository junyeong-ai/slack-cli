//! The text form of every timestamp the CLI writes: RFC 3339 in UTC, with the
//! fraction of a second in groups of three digits and omitted when zero.
//! `auth.json`, the event log and JSON output carry it, and a cache namespace
//! hashes it, so one instant is always written as the same text.

use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serializer};

pub fn format(timestamp: Timestamp) -> String {
    let precision = match timestamp.subsec_nanosecond() {
        0 => 0,
        nanos if nanos % 1_000_000 == 0 => 3,
        nanos if nanos % 1_000 == 0 => 6,
        _ => 9,
    };
    format!("{timestamp:.precision$}")
}

/// Serde adapter for `Timestamp`.
pub mod required {
    use super::*;

    pub fn serialize<S: Serializer>(value: &Timestamp, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&format(*value))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Timestamp, D::Error> {
        Timestamp::deserialize(de)
    }
}

/// Serde adapter for `Option<Timestamp>`.
pub mod option {
    use super::*;

    pub fn serialize<S: Serializer>(value: &Option<Timestamp>, ser: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(timestamp) => ser.serialize_some(&format(*timestamp)),
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Timestamp>, D::Error> {
        Option::<Timestamp>::deserialize(de)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_is_written_in_groups_of_three_digits() {
        for (nanos, written) in [
            (0, "2026-10-17T01:20:00Z"),
            (120_000_000, "2026-10-17T01:20:00.120Z"),
            (123_400_000, "2026-10-17T01:20:00.123400Z"),
            (123_456_000, "2026-10-17T01:20:00.123456Z"),
            (100, "2026-10-17T01:20:00.000000100Z"),
            (123_456_789, "2026-10-17T01:20:00.123456789Z"),
        ] {
            let timestamp = Timestamp::new(1_792_200_000, nanos).unwrap();
            assert_eq!(format(timestamp), written);
            assert_eq!(written.parse::<Timestamp>().unwrap(), timestamp);
        }
    }
}
