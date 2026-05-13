//! Per-spec §7. Three presets cover ~95% of cases — no raw cron input in the UI.

use serde::{Deserialize, Serialize};

/// Wall-clock time-of-day in user's local timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct HhMm {
    pub hour: u8,   // 0-23
    pub minute: u8, // 0-59
}

impl HhMm {
    pub fn new(hour: u8, minute: u8) -> Self {
        debug_assert!(hour < 24 && minute < 60);
        Self { hour, minute }
    }
}

/// Per-account schedule preset.
///
/// Default is `Off`. Stored in `accounts.schedule` as a tagged JSON union:
///   {"type":"Off"}
///   {"type":"Every5h","anchor":{"hour":6,"minute":0}}
///   {"type":"Custom","times":[{"hour":7,"minute":30},{"hour":17,"minute":0}]}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type")]
pub enum Schedule {
    #[default]
    Off,
    Every5h { anchor: HhMm },
    Custom { times: Vec<HhMm> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_round_trips_through_json() {
        let s = Schedule::Off;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"type":"Off"}"#);
        let back: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn every5h_round_trips_through_json() {
        let s = Schedule::Every5h {
            anchor: HhMm::new(6, 0),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"type":"Every5h","anchor":{"hour":6,"minute":0}}"#);
        let back: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn custom_round_trips_through_json() {
        let s = Schedule::Custom {
            times: vec![HhMm::new(7, 30), HhMm::new(17, 0)],
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn legacy_json_in_db_default_value_parses_as_off() {
        let s: Schedule = serde_json::from_str(r#"{"type":"Off"}"#).unwrap();
        assert_eq!(s, Schedule::Off);
    }
}
