//! What the settings card shows for one config folder (spec §7).
//!
//! [`derive_state`] is a pure function of facts the manager gathers, so every
//! state in the table is testable on its own.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

/// "Live" means usage was adopted for a managed account this recently, not
/// merely that the bridge ran.
const LIVE_WINDOW_MIN: i64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeState {
    Off,
    /// No config home and no global config: Claude Code isn't installed here.
    ClaudeCodeNotSetUp,
    /// Enabled, nothing received yet.
    Waiting,
    Live { last_update_ms: i64 },
    /// Enabled and quiet: normal, not a warning.
    Quiet { last_update_ms: Option<i64> },
    /// The bridge runs but no usage was adopted.
    RunningWithoutUsage { reason: String },
    Blocked { reason: String },
    Broken { error: String },
    /// `statusLine.command` no longer runs the bridge.
    ChangedOutside,
    /// `settings.json` can't be edited as it is.
    EditRefused { message: String },
}

/// What the manager knows about a stored, enabled target.
#[derive(Debug, Clone)]
pub struct TargetFacts {
    pub installed_command: String,
    /// Set when the startup probe found the bridge unable to run.
    pub broken: Option<String>,
}

/// What the observation directory says about this config folder.
#[derive(Debug, Clone, Default)]
pub struct ObsSummary {
    pub last_heartbeat_ms: Option<i64>,
    pub last_no_data_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SettingsProbe {
    Ok {
        command: Option<String>,
        disable_all_hooks: bool,
    },
    Refused(String),
}

/// Plain words for why a running bridge produced no usage.
pub fn no_data_reason_text(reason: Option<&str>) -> String {
    match reason {
        Some("no_rate_limits") => "not signed in with a subscription, or no request yet".into(),
        Some("routed") => "routed through a custom endpoint".into(),
        Some("read_error") | Some("incomplete_identity") => "the signed-in account can't be read".into(),
        Some("parse_error") => "its usage data wasn't in a shape Switchboard understands".into(),
        _ => "the account isn't one Switchboard manages, or it is switching accounts".into(),
    }
}

pub fn derive_state(
    target: Option<&TargetFacts>,
    claude_code_set_up: bool,
    obs: &ObsSummary,
    settings: &SettingsProbe,
    last_adopted: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> BridgeState {
    let Some(target) = target else {
        return match (claude_code_set_up, settings) {
            (false, _) => BridgeState::ClaudeCodeNotSetUp,
            // Off, but the file can't be edited as it is: say so now, not
            // only after someone tries.
            (true, SettingsProbe::Refused(message)) => BridgeState::EditRefused { message: message.clone() },
            (true, _) => BridgeState::Off,
        };
    };
    let (command, disable_all_hooks) = match settings {
        SettingsProbe::Refused(message) => {
            return BridgeState::EditRefused { message: message.clone() }
        }
        SettingsProbe::Ok { command, disable_all_hooks } => (command, *disable_all_hooks),
    };
    if command.as_deref() != Some(target.installed_command.as_str()) {
        return BridgeState::ChangedOutside;
    }
    if let Some(error) = &target.broken {
        return BridgeState::Broken { error: error.clone() };
    }
    let window = ChronoDuration::minutes(LIVE_WINDOW_MIN);
    if let Some(at) = last_adopted.filter(|at| now - *at <= window) {
        return BridgeState::Live { last_update_ms: at.timestamp_millis() };
    }
    let fresh_heartbeat = obs
        .last_heartbeat_ms
        .and_then(DateTime::from_timestamp_millis)
        .is_some_and(|at| now - at <= window);
    if fresh_heartbeat {
        return BridgeState::RunningWithoutUsage {
            reason: no_data_reason_text(obs.last_no_data_reason.as_deref()),
        };
    }
    if disable_all_hooks {
        return BridgeState::Blocked {
            reason: "Claude Code's settings turn off status line commands (disableAllHooks).".into(),
        };
    }
    let last = last_adopted
        .map(|at| at.timestamp_millis())
        .or(obs.last_heartbeat_ms);
    match last {
        Some(ms) => BridgeState::Quiet { last_update_ms: Some(ms) },
        None => BridgeState::Waiting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "'/b/bridge' --target 0123456789ab";

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap()
    }

    fn facts() -> TargetFacts {
        TargetFacts { installed_command: CMD.into(), broken: None }
    }

    fn ok(command: &str) -> SettingsProbe {
        SettingsProbe::Ok { command: Some(command.into()), disable_all_hooks: false }
    }

    fn state(target: Option<&TargetFacts>, obs: &ObsSummary, settings: &SettingsProbe, adopted: Option<DateTime<Utc>>) -> BridgeState {
        derive_state(target, true, obs, settings, adopted, now())
    }

    fn ago(mins: i64) -> DateTime<Utc> {
        now() - ChronoDuration::minutes(mins)
    }

    #[test]
    fn off_when_there_is_no_target() {
        assert_eq!(state(None, &ObsSummary::default(), &ok(CMD), None), BridgeState::Off);
    }

    #[test]
    fn off_but_unreadable_settings_say_so() {
        let refused = SettingsProbe::Refused("settings.json isn't valid JSON. Fix it, then try again.".into());
        assert_eq!(
            state(None, &ObsSummary::default(), &refused, None),
            BridgeState::EditRefused { message: "settings.json isn't valid JSON. Fix it, then try again.".into() }
        );
    }

    #[test]
    fn claude_code_not_set_up_when_there_is_no_target_and_no_config() {
        let s = derive_state(None, false, &ObsSummary::default(), &ok(CMD), None, now());
        assert_eq!(s, BridgeState::ClaudeCodeNotSetUp);
    }

    #[test]
    fn waiting_when_enabled_and_nothing_has_arrived() {
        assert_eq!(state(Some(&facts()), &ObsSummary::default(), &ok(CMD), None), BridgeState::Waiting);
    }

    #[test]
    fn live_when_usage_was_adopted_in_the_last_10_minutes() {
        let s = state(Some(&facts()), &ObsSummary::default(), &ok(CMD), Some(ago(9)));
        assert_eq!(s, BridgeState::Live { last_update_ms: ago(9).timestamp_millis() });
    }

    #[test]
    fn quiet_when_adoption_is_older_than_10_minutes() {
        let s = state(Some(&facts()), &ObsSummary::default(), &ok(CMD), Some(ago(120)));
        assert_eq!(s, BridgeState::Quiet { last_update_ms: Some(ago(120).timestamp_millis()) });
    }

    #[test]
    fn running_without_usage_when_heartbeats_arrive_but_nothing_is_adopted() {
        let obs = ObsSummary {
            last_heartbeat_ms: Some(ago(2).timestamp_millis()),
            last_no_data_reason: Some("routed".into()),
        };
        match state(Some(&facts()), &obs, &ok(CMD), None) {
            BridgeState::RunningWithoutUsage { reason } => assert!(reason.contains("custom endpoint")),
            other => panic!("{other:?}"),
        }
        // An old adoption doesn't hide it.
        assert!(matches!(
            state(Some(&facts()), &obs, &ok(CMD), Some(ago(60))),
            BridgeState::RunningWithoutUsage { .. }
        ));
    }

    #[test]
    fn adoption_wins_over_a_heartbeat_with_no_data() {
        let obs = ObsSummary { last_heartbeat_ms: Some(ago(1).timestamp_millis()), last_no_data_reason: Some("no_rate_limits".into()) };
        assert!(matches!(state(Some(&facts()), &obs, &ok(CMD), Some(ago(1))), BridgeState::Live { .. }));
    }

    #[test]
    fn blocked_when_hooks_are_disabled_and_nothing_is_arriving() {
        let settings = SettingsProbe::Ok { command: Some(CMD.into()), disable_all_hooks: true };
        match state(Some(&facts()), &ObsSummary::default(), &settings, None) {
            BridgeState::Blocked { reason } => assert!(reason.contains("disableAllHooks")),
            other => panic!("{other:?}"),
        }
        // Fresh heartbeats prove it isn't blocked after all.
        let obs = ObsSummary { last_heartbeat_ms: Some(ago(1).timestamp_millis()), last_no_data_reason: None };
        assert!(!matches!(state(Some(&facts()), &obs, &settings, None), BridgeState::Blocked { .. }));
    }

    #[test]
    fn broken_when_the_startup_probe_failed() {
        let t = TargetFacts { installed_command: CMD.into(), broken: Some("exec format error".into()) };
        assert_eq!(
            state(Some(&t), &ObsSummary::default(), &ok(CMD), Some(ago(1))),
            BridgeState::Broken { error: "exec format error".into() }
        );
    }

    #[test]
    fn changed_outside_when_the_command_is_no_longer_ours() {
        assert_eq!(state(Some(&facts()), &ObsSummary::default(), &ok("bash my.sh"), None), BridgeState::ChangedOutside);
        let gone = SettingsProbe::Ok { command: None, disable_all_hooks: false };
        assert_eq!(state(Some(&facts()), &ObsSummary::default(), &gone, None), BridgeState::ChangedOutside);
        // It outranks broken: the bridge isn't even in use any more.
        let t = TargetFacts { installed_command: CMD.into(), broken: Some("x".into()) };
        assert_eq!(state(Some(&t), &ObsSummary::default(), &ok("bash my.sh"), None), BridgeState::ChangedOutside);
    }

    #[test]
    fn edit_refused_carries_the_message() {
        let s = SettingsProbe::Refused("settings.json isn't valid JSON. Fix it, then try again.".into());
        assert_eq!(
            state(Some(&facts()), &ObsSummary::default(), &s, None),
            BridgeState::EditRefused { message: "settings.json isn't valid JSON. Fix it, then try again.".into() }
        );
    }
}
