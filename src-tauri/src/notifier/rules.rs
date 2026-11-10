use crate::store::Db;
use crate::usage_api::{ExtraUsage, UsageSnapshot, Utilization};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    FiveHour,
    SevenDay,
    SevenDayFable,
    SevenDayOpus,
    SevenDaySonnet,
    ExtraUsage,
}

impl Bucket {
    pub fn label(&self) -> &'static str {
        match self {
            Bucket::FiveHour => "five_hour",
            Bucket::SevenDay => "seven_day",
            Bucket::SevenDayFable => "seven_day_fable",
            Bucket::SevenDayOpus => "seven_day_opus",
            Bucket::SevenDaySonnet => "seven_day_sonnet",
            Bucket::ExtraUsage => "extra_usage",
        }
    }
    pub fn human(&self) -> &'static str {
        match self {
            Bucket::FiveHour => "5-hour",
            Bucket::SevenDay => "7-day",
            Bucket::SevenDayFable => "7-day Fable",
            Bucket::SevenDayOpus => "7-day Opus",
            Bucket::SevenDaySonnet => "7-day Sonnet",
            Bucket::ExtraUsage => "pay-as-you-go",
        }
    }

    fn window_duration(&self) -> Duration {
        match self {
            Bucket::FiveHour => Duration::hours(5),
            Bucket::SevenDay
            | Bucket::SevenDayFable
            | Bucket::SevenDayOpus
            | Bucket::SevenDaySonnet => Duration::days(7),
            Bucket::ExtraUsage => Duration::days(30),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Fired {
    pub bucket: Bucket,
    pub threshold: u8,
    pub title: String,
    pub body: String,
}

fn utilization_of(bucket: Bucket, s: &UsageSnapshot) -> (Option<f64>, Option<DateTime<Utc>>) {
    fn of(u: &Option<Utilization>) -> (Option<f64>, Option<DateTime<Utc>>) {
        u.as_ref()
            .map(|v| (Some(v.utilization), v.resets_at))
            .unwrap_or((None, None))
    }
    fn ofe(e: &Option<ExtraUsage>) -> (Option<f64>, Option<DateTime<Utc>>) {
        e.as_ref()
            .map(|v| (v.utilization, v.resets_at))
            .unwrap_or((None, None))
    }
    match bucket {
        Bucket::FiveHour => of(&s.five_hour),
        Bucket::SevenDay => of(&s.seven_day),
        Bucket::SevenDayFable => of(&s.seven_day_fable),
        Bucket::SevenDayOpus => of(&s.seven_day_opus),
        Bucket::SevenDaySonnet => of(&s.seven_day_sonnet),
        Bucket::ExtraUsage => ofe(&s.extra_usage),
    }
}

fn humanize_duration(d: Duration) -> String {
    let secs = d.num_seconds().max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

pub fn evaluate(
    db: &Db,
    account_id: &str,
    snapshot: &UsageSnapshot,
    thresholds: &[u8],
    now: DateTime<Utc>,
) -> Result<Vec<Fired>> {
    const BUCKETS: [Bucket; 6] = [
        Bucket::FiveHour,
        Bucket::SevenDay,
        Bucket::SevenDayFable,
        Bucket::SevenDayOpus,
        Bucket::SevenDaySonnet,
        Bucket::ExtraUsage,
    ];
    let mut fired = Vec::new();
    for bucket in BUCKETS {
        let (Some(util), resets_at) = utilization_of(bucket, snapshot) else {
            continue;
        };
        // A snapshot older than its window's reset (adopted from a local
        // source mid-ban) describes a window that is already over.
        if resets_at.is_some_and(|reset| reset <= now) {
            continue;
        }
        for &threshold in thresholds {
            if util < threshold as f64 {
                continue;
            }
            let last = db.notification_last_fired(account_id, bucket.label(), threshold as i64)?;
            let already = match resets_at {
                Some(reset) => {
                    let window_start = reset - bucket.window_duration();
                    last.map(|l| l > window_start).unwrap_or(false)
                }
                None => last
                    .map(|l| (now - l) < Duration::hours(24))
                    .unwrap_or(false),
            };
            if already {
                continue;
            }

            let title = format!("Claude {} usage at {}%", bucket.human(), threshold);
            let body = match (bucket, resets_at) {
                (Bucket::ExtraUsage, None) => "Pay-as-you-go credits running low".to_string(),
                (_, Some(reset)) => format!("Resets in {}", humanize_duration(reset - now)),
                (_, None) => "Window reset time unknown".to_string(),
            };
            db.record_notification_fired(account_id, bucket.label(), threshold as i64, now)?;
            fired.push(Fired {
                bucket,
                threshold,
                title,
                body,
            });
        }
    }
    Ok(fired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Db, StoredAccount};
    use tempfile::tempdir;

    fn fresh() -> (tempfile::TempDir, Db) {
        let d = tempdir().unwrap();
        let db = Db::open(d.path()).unwrap();
        db.upsert_account(&StoredAccount {
            id: "a".into(),
            email: "e".into(),
            display_name: None,
        })
        .unwrap();
        (d, db)
    }

    fn snap_five_hour(util: f64, reset_in_hours: i64) -> UsageSnapshot {
        UsageSnapshot {
            five_hour: Some(Utilization {
                utilization: util,
                resets_at: Some(Utc::now() + Duration::hours(reset_in_hours)),
            }),
            seven_day: None,
            seven_day_sonnet: None,
            seven_day_opus: None,
            seven_day_fable: None,
            extra_usage: None,
            fetched_at: Utc::now(),
            unknown: Default::default(),
        }
    }

    #[test]
    fn fires_once_per_threshold_per_window() {
        let (_d, db) = fresh();
        let s = snap_five_hour(80.0, 3);
        let now = Utc::now();
        let f1 = evaluate(&db, "a", &s, &[75, 90], now).unwrap();
        assert_eq!(f1.len(), 1, "only 75% crosses at 80");
        let f2 = evaluate(
            &db,
            "a",
            &s,
            &[75, 90],
            now + Duration::minutes(5),
        )
        .unwrap();
        assert!(f2.is_empty(), "no re-fire within window");
    }

    #[test]
    fn refires_after_window_reset() {
        let (_d, db) = fresh();
        let now = Utc::now();
        let early = snap_five_hour(80.0, 3);
        evaluate(&db, "a", &early, &[75], now).unwrap();
        let later_reset = Utc::now() + Duration::hours(8);
        let fresh_snap = UsageSnapshot {
            five_hour: Some(Utilization {
                utilization: 80.0,
                resets_at: Some(later_reset),
            }),
            seven_day: None,
            seven_day_sonnet: None,
            seven_day_opus: None,
            seven_day_fable: None,
            extra_usage: None,
            fetched_at: later_reset,
            unknown: Default::default(),
        };
        // Inside the later window (it started at later_reset - 5h, after the
        // first fire) and before that window resets.
        let fired = evaluate(
            &db,
            "a",
            &fresh_snap,
            &[75],
            later_reset - Duration::minutes(1),
        )
        .unwrap();
        assert_eq!(fired.len(), 1);
    }

    #[test]
    fn skips_window_that_has_already_reset() {
        // A snapshot adopted from a local source mid-ban can predate its own
        // window's reset; warning about that window would be about the past.
        let (_d, db) = fresh();
        let s = snap_five_hour(92.0, -1);
        assert!(evaluate(&db, "a", &s, &[90], Utc::now()).unwrap().is_empty());
        assert_eq!(db.notification_last_fired("a", Bucket::FiveHour.label(), 90).unwrap(), None);
    }

    #[test]
    fn extra_usage_without_reset_uses_24h_cooldown() {
        let (_d, db) = fresh();
        let snap = UsageSnapshot {
            five_hour: None,
            seven_day: None,
            seven_day_sonnet: None,
            seven_day_opus: None,
            seven_day_fable: None,
            extra_usage: Some(ExtraUsage {
                is_enabled: true,
                monthly_limit_cents: 5000,
                used_credits_cents: 3750,
                utilization: Some(75.0),
                resets_at: None,
            }),
            fetched_at: Utc::now(),
            unknown: Default::default(),
        };
        let now = Utc::now();
        let a = evaluate(&db, "a", &snap, &[75], now).unwrap();
        assert_eq!(a.len(), 1);
        assert!(a[0].body.contains("credits"));
        let b = evaluate(&db, "a", &snap, &[75], now + Duration::hours(12)).unwrap();
        assert!(b.is_empty(), "inside 24h cooldown");
        let c = evaluate(&db, "a", &snap, &[75], now + Duration::hours(25)).unwrap();
        assert_eq!(c.len(), 1, "past 24h cooldown");
    }

    #[test]
    fn fable_window_has_its_own_notification_bucket() {
        let (_d, db) = fresh();
        let reset = Utc::now() + Duration::days(2);
        let snap = UsageSnapshot {
            five_hour: None,
            seven_day: Some(Utilization {
                utilization: 50.0,
                resets_at: Some(reset),
            }),
            seven_day_sonnet: None,
            seven_day_opus: None,
            seven_day_fable: Some(Utilization {
                utilization: 78.0,
                resets_at: Some(reset),
            }),
            extra_usage: None,
            fetched_at: Utc::now(),
            unknown: Default::default(),
        };
        let fired = evaluate(&db, "a", &snap, &[75], Utc::now()).unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].bucket, Bucket::SevenDayFable);
        assert!(fired[0].title.contains("Fable"));
    }

    #[test]
    fn below_threshold_does_not_fire() {
        let (_d, db) = fresh();
        let s = snap_five_hour(50.0, 3);
        assert!(evaluate(&db, "a", &s, &[75, 90], Utc::now())
            .unwrap()
            .is_empty());
    }
}
