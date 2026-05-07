use claude_switchboard_lib::jsonl_parser::record::parse_event_line;

#[test]
fn current_schema_parses_every_line() {
    let raw = include_str!("fixtures/jsonl/current_schema.jsonl");
    let events: Vec<_> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| parse_event_line(l, "fallback").expect("parse"))
        .collect();
    assert_eq!(events.len(), 3);
    assert_eq!(events[1].model, "claude-opus-4-7-20260115");
    assert_eq!(events[0].cache_read_tokens, 200);
}

#[test]
fn older_schema_with_unknown_fields_still_parses() {
    let raw = include_str!("fixtures/jsonl/older_schema.jsonl");
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let e = parse_event_line(line, "fallback").expect("parse older");
        assert!(!e.project.is_empty());
    }
}

#[test]
fn malformed_lines_are_individually_rejectable() {
    let raw = include_str!("fixtures/jsonl/malformed_lines.jsonl");
    let (ok, err): (Vec<_>, Vec<_>) = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .partition(|l| parse_event_line(l, "fallback").is_some());
    assert_eq!(ok.len(), 3);
    assert_eq!(err.len(), 2);
}
