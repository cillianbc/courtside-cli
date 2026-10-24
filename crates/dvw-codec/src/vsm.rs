//! VolleyStation `.vsm` files at the JSON level.
//!
//! A `.vsm` is compact UTF-8 JSON with no trailing newline. Every corpus file
//! re-serialises byte-exact when key order is preserved, so the raw value is
//! the lossless layer here; typed reading of sets and plays builds on it.

use serde_json::Value;

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct RawVsm {
    pub root: Value,
}

impl RawVsm {
    pub fn parse(input: &[u8]) -> Result<Self, Error> {
        Ok(RawVsm {
            root: serde_json::from_slice(input)?,
        })
    }

    /// Compact JSON in the source key order, matching VolleyStation's output.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Serialising a `Value` to a Vec cannot fail: keys are always strings.
        serde_json::to_vec(&self.root).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_compact_json_in_key_order() {
        // VolleyStation writes non-ASCII as raw UTF-8 and escapes control chars.
        let input =
            r#"{"gameType":"indoor","version":1,"b":[1,-2,{"z":"Hélène\t","a":null}],"a":true}"#;
        let vsm = RawVsm::parse(input.as_bytes()).unwrap();
        assert_eq!(vsm.to_bytes(), input.as_bytes());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(RawVsm::parse(b"{\"scout\":").is_err());
    }
}
