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
