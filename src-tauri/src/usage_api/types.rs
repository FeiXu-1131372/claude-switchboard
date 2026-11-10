use chrono::{DateTime, TimeZone, Utc};
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct Utilization {
    pub utilization: f64,
    // The API returns `resets_at: null` for buckets that exist with
    // `utilization: 0.0` but have no active window yet (e.g. an unused
    // `seven_day_sonnet`). Keeping it required would fail the whole snapshot
    // decode whenever any sibling bucket is idle.
    #[serde(default)]
    #[specta(type = Option<String>)]
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct ExtraUsage {
    #[serde(default)]
    pub is_enabled: bool,
    #[serde(default)]
    pub monthly_limit_cents: u64,
    #[serde(default)]
    pub used_credits_cents: u64,
    // Sent as `null` when extra usage is not enabled; `#[serde(default)]`
    // alone would not cover that since the field is present-but-null.
    #[serde(default)]
    pub utilization: Option<f64>,
    #[serde(default)]
    #[specta(type = Option<String>)]
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct UsageSnapshot {
    pub five_hour: Option<Utilization>,
    pub seven_day: Option<Utilization>,
    pub seven_day_sonnet: Option<Utilization>,
    pub seven_day_opus: Option<Utilization>,
    /// Model-scoped weekly limit for Fable, sourced from the modern
    /// `limits[]` array or a legacy alias. Only Max and Ultra come with
    /// Fable, but the API sends the row to every plan: see
    /// [`UsageSnapshot::hide_fable_without_access`].
    pub seven_day_fable: Option<Utilization>,
    pub extra_usage: Option<ExtraUsage>,

    #[specta(type = String)]
    pub fetched_at: DateTime<Utc>,

    // Carries forward unknown fields from the wire so we never reject a payload
    // that the API extends; not exposed to the frontend.
    #[serde(skip_serializing)]
    #[specta(skip)]
    pub unknown: HashMap<String, Value>,
}

impl UsageSnapshot {
    /// Drop the Fable row of an account that cannot use Fable.
    ///
    /// `/usage` sends the Fable weekly row to every account, Pro included,
    /// and nothing in the row says whether the plan comes with Fable: a Pro
    /// account's row and an untouched Ultra account's row are identical (0%,
    /// `is_active: false`). The plan is the only thing that tells them apart.
    ///
    /// A row that shows usage is always kept. Usage proves access, so a plan
    /// label that is unknown or out of date can never hide a limit the
    /// account is actually running down.
    pub fn hide_fable_without_access(&mut self, plan_includes_fable: bool) {
        let unused = self
            .seven_day_fable
            .as_ref()
            .is_some_and(|fable| fable.utilization <= 0.0);
        if unused && !plan_includes_fable {
            self.seven_day_fable = None;
        }
    }
}

impl<'de> Deserialize<'de> for UsageSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(mut obj) = value else {
            return Err(de::Error::custom("usage snapshot must be a JSON object"));
        };

        let mut five_hour = take_window(&mut obj, "five_hour").map_err(de::Error::custom)?;
        let mut seven_day = take_window(&mut obj, "seven_day").map_err(de::Error::custom)?;
        let mut seven_day_sonnet =
            take_window(&mut obj, "seven_day_sonnet").map_err(de::Error::custom)?;
        let mut seven_day_opus =
            take_window(&mut obj, "seven_day_opus").map_err(de::Error::custom)?;
        // The Fable window had several spellings before `limits[]` existed.
        // A malformed legacy alias must not reject the whole snapshot.
        let mut seven_day_fable = take_window_lossy(&mut obj, "seven_day_fable");
        if seven_day_fable.is_none() {
            seven_day_fable = take_window_lossy(&mut obj, "fable_weekly")
                .or_else(|| take_window_lossy(&mut obj, "fable_seven_day"));
        }

        let extra_usage = take_value(&mut obj, "extra_usage")
            .map(|v| serde_json::from_value::<ExtraUsage>(v).map_err(de::Error::custom))
            .transpose()?;

        let fetched_at = match take_value(&mut obj, "fetched_at") {
            Some(v) => parse_reset(Some(v))
                .map_err(de::Error::custom)?
                .unwrap_or_else(Utc::now),
            None => Utc::now(),
        };

        // Modern model-scoped limits. Entries are parsed independently: an
        // unknown or malformed row is skipped rather than failing the whole
        // snapshot. Modern rows override the legacy top-level windows.
        if let Some(Value::Array(limits)) = obj.remove("limits") {
            for raw in limits {
                let Ok(entry) = serde_json::from_value::<LimitEntry>(raw) else {
                    continue;
                };
                let Some(utilization) = entry.utilization() else {
                    continue;
                };
                match entry.kind.as_deref() {
                    Some("session") if entry.is_unscoped() => five_hour = Some(utilization),
                    Some("weekly_all") if entry.is_unscoped() => seven_day = Some(utilization),
                    Some("weekly_scoped") if entry.is_weekly_scoped() => {
                        let Some(model) = entry.model_name() else {
                            continue;
                        };
                        match model.as_str() {
                            "fable" => seven_day_fable = Some(utilization),
                            "opus" => seven_day_opus = Some(utilization),
                            "sonnet" => seven_day_sonnet = Some(utilization),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }

        let unknown: HashMap<String, Value> = obj.into_iter().collect();
        Ok(Self {
            five_hour,
            seven_day,
            seven_day_sonnet,
            seven_day_opus,
            seven_day_fable,
            extra_usage,
            fetched_at,
            unknown,
        })
    }
}

#[derive(Debug, Deserialize)]
struct LimitEntry {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    percent: Option<f64>,
    #[serde(default)]
    resets_at: Option<Value>,
    #[serde(default)]
    scope: Option<LimitScope>,
}

#[derive(Debug, Deserialize)]
struct LimitScope {
    #[serde(default)]
    model: Option<LimitModel>,
    #[serde(default)]
    surface: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct LimitModel {
    #[serde(default)]
    display_name: Option<String>,
}

impl LimitEntry {
    fn utilization(&self) -> Option<Utilization> {
        let utilization = self.percent?;
        if !utilization.is_finite() || utilization < 0.0 {
            return None;
        }
        let resets_at = parse_reset(self.resets_at.clone()).ok()?;
        Some(Utilization {
            utilization,
            resets_at,
        })
    }

    fn is_unscoped(&self) -> bool {
        match &self.scope {
            None => true,
            Some(scope) => {
                scope.model.is_none()
                    && scope
                        .surface
                        .as_ref()
                        .is_none_or(|surface| surface.is_null())
            }
        }
    }

    fn is_weekly_scoped(&self) -> bool {
        self.group.as_deref().is_none_or(|group| group == "weekly")
            && self
                .scope
                .as_ref()
                .and_then(|scope| scope.surface.as_ref())
                .is_none_or(|surface| surface.is_null())
    }

    fn model_name(&self) -> Option<String> {
        self.scope
            .as_ref()?
            .model
            .as_ref()?
            .display_name
            .as_deref()
            .map(|name| {
                name.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_ascii_lowercase()
            })
    }
}

fn take_value(obj: &mut Map<String, Value>, key: &str) -> Option<Value> {
    obj.remove(key).filter(|value| !value.is_null())
}

fn take_window(obj: &mut Map<String, Value>, key: &str) -> Result<Option<Utilization>, String> {
    match take_value(obj, key) {
        None => Ok(None),
        Some(value) => parse_window(value).map(Some),
    }
}

fn take_window_lossy(obj: &mut Map<String, Value>, key: &str) -> Option<Utilization> {
    take_value(obj, key).and_then(|value| parse_window(value).ok())
}

fn parse_window(value: Value) -> Result<Utilization, String> {
    #[derive(Deserialize)]
    struct RawWindow {
        #[serde(default)]
        utilization: Option<f64>,
        #[serde(default)]
        used_percentage: Option<f64>,
        #[serde(default)]
        resets_at: Option<Value>,
    }

    let raw: RawWindow = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let utilization = raw
        .utilization
        .or(raw.used_percentage)
        .filter(|value| value.is_finite())
        .ok_or_else(|| "usage window missing a numeric utilization".to_string())?;
    Ok(Utilization {
        utilization,
        resets_at: parse_reset(raw.resets_at)?,
    })
}

fn parse_reset(value: Option<Value>) -> Result<Option<DateTime<Utc>>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            if let Ok(epoch) = text.parse::<i64>() {
                return parse_epoch(epoch);
            }
            DateTime::parse_from_rfc3339(text)
                .map(|value| Some(value.with_timezone(&Utc)))
                .map_err(|e| e.to_string())
        }
        Some(Value::Number(number)) => {
            let epoch = number
                .as_i64()
                .or_else(|| number.as_f64().map(|value| value as i64))
                .ok_or_else(|| "non-integer timestamp".to_string())?;
            parse_epoch(epoch)
        }
        Some(_) => Err("resets_at must be a timestamp string or number".to_string()),
    }
}

fn parse_epoch(epoch: i64) -> Result<Option<DateTime<Utc>>, String> {
    // The API has emitted both seconds and milliseconds.
    let seconds = if epoch.abs() >= 1_000_000_000_000 {
        epoch / 1000
    } else {
        epoch
    };
    Utc.timestamp_opt(seconds, 0)
        .single()
        .map(Some)
        .ok_or_else(|| "timestamp out of range".to_string())
}
