use crate::tray_icon;
use chrono::{DateTime, Utc};
use tauri::image::Image;
use tauri::AppHandle;

/// Live values rendered by the tray icon and tooltip. Grouped so adding a
/// model-scoped window does not grow the updater's argument list.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrayUsage {
    pub five_hour: Option<f64>,
    pub seven_day: Option<f64>,
    pub seven_day_fable: Option<f64>,
    pub five_hour_resets_at: Option<DateTime<Utc>>,
    pub seven_day_resets_at: Option<DateTime<Utc>>,
    pub seven_day_fable_resets_at: Option<DateTime<Utc>>,
}

impl From<&crate::usage_api::UsageSnapshot> for TrayUsage {
    fn from(snapshot: &crate::usage_api::UsageSnapshot) -> Self {
        let percent = |window: &Option<crate::usage_api::Utilization>| {
            window.as_ref().map(|u| u.utilization)
        };
        let resets_at = |window: &Option<crate::usage_api::Utilization>| {
            window.as_ref().and_then(|u| u.resets_at)
        };
        Self {
            five_hour: percent(&snapshot.five_hour),
            seven_day: percent(&snapshot.seven_day),
            seven_day_fable: percent(&snapshot.seven_day_fable),
            five_hour_resets_at: resets_at(&snapshot.five_hour),
            seven_day_resets_at: resets_at(&snapshot.seven_day),
            seven_day_fable_resets_at: resets_at(&snapshot.seven_day_fable),
        }
    }
}

/// Updates the tray icon. The icon is synthesized on every call from the
/// live percentages — no static asset paths are involved.
///
/// The reset timestamps are absolute; the tooltip formats them as
/// "resets in Xh Ym" so the user can answer "can I keep coding?" at a glance.
pub fn set_level(app: &AppHandle, usage: TrayUsage, paused: bool) {
    let Some(tray) = app.tray_by_id("main") else { return };

    let bytes = tray_icon::render(usage.five_hour, usage.seven_day, paused);
    match Image::from_bytes(&bytes) {
        Ok(img) => { let _ = tray.set_icon_with_as_template(Some(img), false); }
        Err(e) => tracing::warn!("tray icon decode failed, keeping previous icon: {e}"),
    }

    // Numbers live in the icon now — no separate title text alongside.
    let _ = tray.set_title(None::<&str>);
    let tooltip = tooltip(usage, paused, Utc::now());

    // A global provider default means the usage bars no longer describe where
    // the user's work is going. Name it rather than let the bars mislead.
    // Sessions launched from the Providers tab get no marker — those are
    // explicit acts in visible windows, and several may run at once.
    let tooltip = match default_provider_name(app) {
        Some(name) => format!("{tooltip}\nDefault provider: {name}"),
        None => tooltip,
    };

    let _ = tray.set_tooltip(Some(tooltip));
}

/// Name of the provider currently set as the global default, if any.
fn default_provider_name(app: &AppHandle) -> Option<String> {
    use tauri::Manager;
    let state = app.try_state::<std::sync::Arc<crate::app_state::AppState>>()?;
    let default = state.db.get_default_provider().ok().flatten()?;
    Some(
        state
            .db
            .get_provider(&default.provider_id)
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_else(|| "a custom provider".to_string()),
    )
}

fn tooltip(usage: TrayUsage, paused: bool, now: DateTime<Utc>) -> String {
    let TrayUsage {
        five_hour,
        seven_day,
        seven_day_fable,
        five_hour_resets_at,
        seven_day_resets_at,
        seven_day_fable_resets_at,
    } = usage;
    if paused && five_hour.is_none() && seven_day.is_none() && seven_day_fable.is_none() {
        return "Claude usage — sign-in required".into();
    }

    if five_hour.is_none() && seven_day.is_none() && seven_day_fable.is_none() {
        return "Claude usage".into();
    }

    let fmt_reset = |reset: Option<DateTime<Utc>>| -> String {
        let Some(r) = reset else { return String::new() };
        let minutes = (r - now).num_minutes().max(0);
        let d = minutes / (60 * 24);
        let h = (minutes % (60 * 24)) / 60;
        let m = minutes % 60;
        let reset_str = if d > 0 {
            format!("{}d {}h", d, h)
        } else if h > 0 {
            format!("{}h {}m", h, m)
        } else {
            format!("{}m", m)
        };
        format!(" — resets in {}", reset_str)
    };

    let mut lines: Vec<String> = Vec::new();
    if let Some(p) = five_hour {
        lines.push(format!("5h: {}%{}", p.round() as i64, fmt_reset(five_hour_resets_at)));
    }
    if let Some(p) = seven_day {
        lines.push(format!("7d: {}%{}", p.round() as i64, fmt_reset(seven_day_resets_at)));
    }
    if let Some(p) = seven_day_fable {
        lines.push(format!(
            "Fable: {}%{}",
            p.round() as i64,
            fmt_reset(seven_day_fable_resets_at)
        ));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(min_from_now: i64) -> DateTime<Utc> {
        let now: DateTime<Utc> = "2026-04-27T12:00:00Z".parse().unwrap();
        now + chrono::Duration::minutes(min_from_now)
    }
    fn now() -> DateTime<Utc> {
        "2026-04-27T12:00:00Z".parse().unwrap()
    }

    #[test]
    fn tooltip_full_with_reset_in_hours_and_minutes() {
        let s = tooltip(
            TrayUsage {
                five_hour: Some(63.0),
                seven_day: Some(28.0),
                five_hour_resets_at: Some(t(134)),
                seven_day_resets_at: Some(t(134 + 5 * 60)),
                ..Default::default()
            },
            false,
            now(),
        );
        assert_eq!(s, "5h: 63% — resets in 2h 14m\n7d: 28% — resets in 7h 14m");
    }

    #[test]
    fn tooltip_full_with_reset_under_an_hour() {
        let s = tooltip(
            TrayUsage {
                five_hour: Some(63.0),
                seven_day: Some(28.0),
                five_hour_resets_at: Some(t(45)),
                ..Default::default()
            },
            false,
            now(),
        );
        assert_eq!(s, "5h: 63% — resets in 45m\n7d: 28%");
    }

    #[test]
    fn tooltip_clamps_negative_reset_to_zero() {
        let s = tooltip(
            TrayUsage {
                five_hour: Some(99.0),
                seven_day: Some(50.0),
                five_hour_resets_at: Some(t(-10)),
                ..Default::default()
            },
            false,
            now(),
        );
        assert_eq!(s, "5h: 99% — resets in 0m\n7d: 50%");
    }

    #[test]
    fn tooltip_paused_with_no_data_shows_signin_prompt() {
        let s = tooltip(TrayUsage::default(), true, now());
        assert_eq!(s, "Claude usage — sign-in required");
    }

    #[test]
    fn tooltip_drops_missing_pieces() {
        // Partial data — only 7d known, no 5h reading and no reset time.
        let s = tooltip(
            TrayUsage {
                seven_day: Some(28.0),
                ..Default::default()
            },
            false,
            now(),
        );
        assert_eq!(s, "7d: 28%");
    }

    #[test]
    fn tooltip_unpaused_with_nothing_falls_back_to_generic() {
        let s = tooltip(TrayUsage::default(), false, now());
        assert_eq!(s, "Claude usage");
    }

    #[test]
    fn tooltip_both_resets_shown() {
        // 135 min → 2h 15m; 2880 min (48h) → 2d 0h
        let s = tooltip(
            TrayUsage {
                five_hour: Some(21.0),
                seven_day: Some(50.0),
                five_hour_resets_at: Some(t(135)),
                seven_day_resets_at: Some(t(2880)),
                ..Default::default()
            },
            false,
            now(),
        );
        assert_eq!(s, "5h: 21% — resets in 2h 15m\n7d: 50% — resets in 2d 0h");
    }

    #[test]
    fn tooltip_includes_fable_when_present() {
        let s = tooltip(
            TrayUsage {
                five_hour: Some(63.0),
                seven_day: Some(28.0),
                seven_day_fable: Some(44.0),
                five_hour_resets_at: Some(t(134)),
                seven_day_resets_at: Some(t(134)),
                seven_day_fable_resets_at: Some(t(300)),
            },
            false,
            now(),
        );
        assert_eq!(
            s,
            "5h: 63% — resets in 2h 14m\n7d: 28% — resets in 2h 14m\nFable: 44% — resets in 5h 0m"
        );
    }
}
