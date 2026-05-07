//! Catch-up sweep on app launch (per spec §7).
//!
//! `last_expected_fire` is COMPUTED, not stored. This module computes it
//! from the schedule shape with a 24h lookback floor.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone};

use super::presets::{HhMm, Schedule};

const LOOKBACK_HOURS: i64 = 24;

/// Materialize a candidate fire time `(date, hh:mm)` in the user's local TZ.
/// Returns None if the time is invalid for that date (e.g. spring-forward gap).
fn local_at(date: NaiveDate, hhmm: HhMm) -> Option<DateTime<Local>> {
    let t = NaiveTime::from_hms_opt(hhmm.hour as u32, hhmm.minute as u32, 0)?;
    let naive = date.and_time(t);
    Local.from_local_datetime(&naive).single()
}

/// Compute the most-recent expected fire time at-or-before `now`, scanning
/// back at most 24 hours. Returns None if the schedule is `Off` or no
/// occurrence falls within the lookback.
pub fn most_recent_expected_fire(
    schedule: &Schedule,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let lookback_floor = now - Duration::hours(LOOKBACK_HOURS);
    let mut candidates: Vec<DateTime<Local>> = Vec::new();

    let today = now.date_naive();
    let yesterday = today.pred_opt()?;

    let push_for_hhmm = |c: &mut Vec<DateTime<Local>>, hhmm: HhMm| {
        for d in [yesterday, today] {
            if let Some(dt) = local_at(d, hhmm) {
                if dt > lookback_floor && dt <= now {
                    c.push(dt);
                }
            }
        }
    };

    match schedule {
        Schedule::Off => return None,
        Schedule::Every5h { anchor } => {
            // Anchor + (k * 5h) for k in 0..5, generating up to 5 fires per day.
            for k in 0..5 {
                let total_min = (anchor.hour as u32) * 60 + (anchor.minute as u32) + 5 * 60 * k;
                let h = (total_min / 60) % 24;
                let m = total_min % 60;
                let hhmm = HhMm::new(h as u8, m as u8);
                push_for_hhmm(&mut candidates, hhmm);
            }
        }
        Schedule::Custom { times } => {
            for hhmm in times {
                push_for_hhmm(&mut candidates, *hhmm);
            }
        }
    }

    candidates.into_iter().max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid datetime")
    }

    #[test]
    fn off_returns_none() {
        let now = local(2026, 5, 7, 14, 0);
        assert_eq!(most_recent_expected_fire(&Schedule::Off, now), None);
    }

    #[test]
    fn every5h_06_anchor_at_1430_returns_1100() {
        let s = Schedule::Every5h {
            anchor: HhMm::new(6, 0),
        };
        let now = local(2026, 5, 7, 14, 30);
        let got = most_recent_expected_fire(&s, now).unwrap();
        let want = local(2026, 5, 7, 11, 0);
        assert_eq!(got, want);
    }

    #[test]
    fn every5h_06_anchor_at_0530_returns_todays_0200() {
        // anchor=06:00, k=4 → 06+20h = 26h → wraps to 02:00 next day.
        // With now=05:30, the 24h lookback floor is yesterday 05:30.
        // Fires in window: yesterday 06, 11, 16, 21 and today 02:00.
        // today 02:00 is the most-recent (more recent than yesterday 21:00).
        let s = Schedule::Every5h {
            anchor: HhMm::new(6, 0),
        };
        let now = local(2026, 5, 7, 5, 30);
        let got = most_recent_expected_fire(&s, now).unwrap();
        let want = local(2026, 5, 7, 2, 0); // today's 06+20h % 24 = 02:00
        assert_eq!(got, want);
    }

    #[test]
    fn custom_picks_latest_within_lookback() {
        let s = Schedule::Custom {
            times: vec![HhMm::new(7, 30), HhMm::new(12, 0), HhMm::new(17, 0)],
        };
        let now = local(2026, 5, 7, 14, 30);
        let got = most_recent_expected_fire(&s, now).unwrap();
        let want = local(2026, 5, 7, 12, 0);
        assert_eq!(got, want);
    }

    #[test]
    fn lookback_handles_24h_floor() {
        let s = Schedule::Custom {
            times: vec![HhMm::new(9, 0)],
        };
        let now = local(2026, 5, 7, 9, 30);
        let got = most_recent_expected_fire(&s, now).unwrap();
        assert_eq!(got, local(2026, 5, 7, 9, 0));
    }
}
