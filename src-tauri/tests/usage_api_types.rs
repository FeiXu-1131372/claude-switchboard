use claude_switchboard_lib::usage_api::types::UsageSnapshot;

#[test]
fn standard_account_round_trips() {
    let raw = include_str!("fixtures/api_responses/standard_account.json");
    let snap: UsageSnapshot = serde_json::from_str(raw).expect("parse");
    assert!(snap.five_hour.is_some());
    assert!(snap.extra_usage.is_none());
    let back = serde_json::to_string(&snap).unwrap();
    let reparsed: UsageSnapshot = serde_json::from_str(&back).unwrap();
    assert_eq!(snap.five_hour, reparsed.five_hour);
}

#[test]
fn extra_usage_enabled_parses() {
    let raw = include_str!("fixtures/api_responses/extra_usage_enabled.json");
    let snap: UsageSnapshot = serde_json::from_str(raw).expect("parse");
    let eu = snap.extra_usage.expect("extra_usage");
    assert!(eu.is_enabled);
    assert_eq!(eu.monthly_limit_cents, 5000);
    assert_eq!(eu.used_credits_cents, 1275);
    assert!(eu.resets_at.is_some());
}

#[test]
fn unknown_fields_are_preserved_not_errors() {
    let raw = include_str!("fixtures/api_responses/newer_schema_with_extra_fields.json");
    let snap: UsageSnapshot = serde_json::from_str(raw).expect("parse forward-compat");
    assert!(snap.unknown.contains_key("future_field_we_do_not_know"));
    assert!(snap.unknown.contains_key("organization_plan"));
}

#[test]
fn scoped_weekly_limits_parse_fable_and_override_legacy_windows() {
    let raw = r#"{
        "five_hour": null,
        "seven_day": { "utilization": 96.0, "resets_at": "2026-11-13T01:00:00Z" },
        "seven_day_sonnet": { "utilization": 10.0, "resets_at": null },
        "seven_day_opus": { "utilization": 11.0, "resets_at": null },
        "seven_day_fable": { "utilization": 50.0, "resets_at": null },
        "extra_usage": null,
        "limits": [
            {
                "kind": "weekly_scoped",
                "group": "weekly",
                "percent": 37.5,
                "resets_at": "2026-11-13T01:00:00Z",
                "is_active": true,
                "scope": {
                    "model": { "id": null, "display_name": "  fAbLe  " },
                    "surface": null
                }
            },
            {
                "kind": "weekly_scoped",
                "group": "weekly",
                "percent": 22.0,
                "resets_at": null,
                "scope": { "model": { "display_name": "Opus" }, "surface": null }
            },
            {
                "kind": "weekly_scoped",
                "group": "weekly",
                "percent": 33.0,
                "resets_at": null,
                "scope": { "model": { "display_name": "Sonnet" }, "surface": null }
            }
        ]
    }"#;

    let snap: UsageSnapshot = serde_json::from_str(raw).expect("parse modern limits");
    let fable = snap.seven_day_fable.expect("fable");
    assert!((fable.utilization - 37.5).abs() < 0.001);
    assert!(fable.resets_at.is_some());
    let opus = snap.seven_day_opus.expect("opus");
    assert!((opus.utilization - 22.0).abs() < 0.001);
    let sonnet = snap.seven_day_sonnet.expect("sonnet");
    assert!((sonnet.utilization - 33.0).abs() < 0.001);
}

#[test]
fn unscoped_modern_limits_override_the_legacy_general_windows() {
    let raw = r#"{
        "five_hour": { "utilization": 1.0, "resets_at": null },
        "seven_day": { "utilization": 2.0, "resets_at": null },
        "seven_day_sonnet": null,
        "seven_day_opus": null,
        "extra_usage": null,
        "limits": [
            {
                "kind": "session",
                "group": "session",
                "percent": 12.0,
                "resets_at": "2026-11-13T01:00:00Z",
                "scope": null
            },
            {
                "kind": "weekly_all",
                "group": "weekly",
                "percent": 34.0,
                "resets_at": "2026-11-15T01:00:00Z",
                "scope": null
            }
        ]
    }"#;

    let snap: UsageSnapshot = serde_json::from_str(raw).expect("parse modern general limits");
    assert!((snap.five_hour.expect("five hour").utilization - 12.0).abs() < 0.001);
    assert!((snap.seven_day.expect("seven day").utilization - 34.0).abs() < 0.001);
}

#[test]
fn legacy_fable_alias_and_epoch_reset_are_supported() {
    let raw = r#"{
        "five_hour": null,
        "seven_day": null,
        "seven_day_sonnet": null,
        "seven_day_opus": null,
        "fable_weekly": { "used_percentage": 42.0, "resets_at": 1794625200 },
        "extra_usage": null
    }"#;

    let snap: UsageSnapshot = serde_json::from_str(raw).expect("parse legacy fable alias");
    let fable = snap.seven_day_fable.expect("fable");
    assert!((fable.utilization - 42.0).abs() < 0.001);
    assert!(fable.resets_at.is_some());
}

#[test]
fn malformed_limits_entries_are_skipped_without_failing_the_snapshot() {
    let raw = r#"{
        "five_hour": null,
        "seven_day": null,
        "seven_day_sonnet": null,
        "seven_day_opus": null,
        "extra_usage": null,
        "limits": [
            { "kind": "weekly_scoped", "percent": "not a number" },
            {
                "kind": "weekly_scoped",
                "group": "weekly",
                "percent": 80.0,
                "scope": { "model": { "display_name": "Fable" }, "surface": "cli" }
            },
            {
                "kind": "weekly_scoped",
                "group": "weekly",
                "percent": 25.0,
                "scope": { "model": { "display_name": "Fable" }, "surface": null }
            }
        ],
        "future_field": { "kept": true }
    }"#;

    let snap: UsageSnapshot = serde_json::from_str(raw).expect("malformed rows are skipped");
    assert!((snap.seven_day_fable.expect("fable").utilization - 25.0).abs() < 0.001);
    assert!(snap.unknown.contains_key("future_field"));
}

/// What `/usage` returned on 11 Nov 2026 for a Pro account and for an Ultra
/// account that had not touched Fable: the same Fable row, 0% and
/// `is_active: false`. Nothing in the payload tells the two apart.
fn snapshot_with_fable_row(percent: f64) -> UsageSnapshot {
    let raw = format!(
        r#"{{
            "five_hour": {{ "utilization": 0.0, "resets_at": "2026-11-10T23:09:59.893287+00:00" }},
            "seven_day": {{ "utilization": 85.0, "resets_at": "2026-11-15T14:59:59.893308+00:00" }},
            "seven_day_opus": null,
            "seven_day_sonnet": null,
            "extra_usage": null,
            "limits": [
                {{ "kind": "session", "group": "session", "percent": 0, "severity": "normal",
                  "resets_at": "2026-11-10T23:09:59.893287+00:00", "scope": null, "is_active": false }},
                {{ "kind": "weekly_all", "group": "weekly", "percent": 85, "severity": "warning",
                  "resets_at": "2026-11-15T14:59:59.893308+00:00", "scope": null, "is_active": true }},
                {{ "kind": "weekly_scoped", "group": "weekly", "percent": {percent}, "severity": "normal",
                  "resets_at": "2026-11-15T15:00:00+00:00",
                  "scope": {{ "model": {{ "id": null, "display_name": "Fable" }}, "surface": null }},
                  "is_active": false }}
            ],
            "weekly_scoped_shares": [
                {{ "limit_index": 2, "used_percent_of_weekly": 0, "allowance_percent_of_weekly": 50 }}
            ]
        }}"#
    );
    serde_json::from_str(&raw).expect("parse real-shape payload")
}

#[test]
fn the_api_sends_a_fable_row_even_to_plans_without_fable() {
    assert!(snapshot_with_fable_row(0.0).seven_day_fable.is_some());
}

#[test]
fn an_unused_fable_row_is_hidden_when_the_plan_has_no_fable() {
    let mut snap = snapshot_with_fable_row(0.0);
    snap.hide_fable_without_access(false);
    assert!(snap.seven_day_fable.is_none());
    // The general windows are untouched.
    assert!(snap.five_hour.is_some() && snap.seven_day.is_some());
}

#[test]
fn an_unused_fable_row_stays_when_the_plan_includes_fable() {
    let mut snap = snapshot_with_fable_row(0.0);
    snap.hide_fable_without_access(true);
    assert_eq!(snap.seven_day_fable.expect("fable").utilization, 0.0);
}

#[test]
fn fable_usage_is_shown_even_if_the_plan_label_says_otherwise() {
    // Usage proves access. A stale plan label must never hide a limit the
    // account is actually running down.
    let mut snap = snapshot_with_fable_row(45.0);
    snap.hide_fable_without_access(false);
    assert_eq!(snap.seven_day_fable.expect("fable").utilization, 45.0);
}
