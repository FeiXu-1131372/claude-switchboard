import { describe, it, expect } from 'vitest';
import { compactionRowId, isHeadlessProject, modelLabel } from './SessionsTab';

describe('compactionRowId', () => {
  // The Cost tab looks compactions up by the row id the backend gives each
  // row, `<parent transcript>#<day>`. A compaction recorded in a subagent's
  // transcript has to fold onto its parent the same way, or the marker
  // silently never renders. 11:00 UTC is the 26th from UTC-11 to UTC+12.
  it('is the id of the row for that conversation and day', () => {
    expect(compactionRowId({ source_file: '/p/x/s1.jsonl', ts: '2026-07-26T11:00:00Z' })).toBe(
      '/p/x/s1.jsonl#2026-07-26',
    );
  });

  it('puts a compaction inside a subagent on the session that started it', () => {
    expect(
      compactionRowId({ source_file: '/p/x/s1/subagents/agent-aaa.jsonl', ts: '2026-07-26T11:00:00Z' }),
    ).toBe('/p/x/s1.jsonl#2026-07-26');
  });
});

describe('modelLabel', () => {
  it('collapses Anthropic families to their tier name', () => {
    expect(modelLabel('claude-opus-5-5')).toBe('opus');
    expect(modelLabel('claude-opus-4-7')).toBe('opus');
    expect(modelLabel('claude-sonnet-5-5')).toBe('sonnet');
    expect(modelLabel('claude-sonnet-5')).toBe('sonnet');
    expect(modelLabel('claude-haiku-4-5-20251001')).toBe('haiku');
    expect(modelLabel('claude-fable-5-1')).toBe('fable');
    expect(modelLabel('claude-fable-5')).toBe('fable');
  });
  it('surfaces the real name for third-party / relay models', () => {
    expect(modelLabel('glm-5.3')).toBe('glm-5.3');
    expect(modelLabel('glm-5.2')).toBe('glm-5.2');
    expect(modelLabel('k3')).toBe('k3');
    expect(modelLabel('deepseek-v4.1-flash-1017')).toBe('deepseek-v4.1-flash-1017');
    expect(modelLabel('MiniMax-M2.7-highspeed')).toBe('MiniMax-M2.7');
    expect(modelLabel('kimi-for-coding-highspeed')).toBe('kimi');
  });
});

describe('isHeadlessProject', () => {
  it('flags the no-cwd bucket ("-") and empty', () => {
    expect(isHeadlessProject('-')).toBe(true);
    expect(isHeadlessProject('')).toBe(true);
  });
  it('leaves real project names alone', () => {
    expect(isHeadlessProject('life-os')).toBe(false);
    expect(isHeadlessProject('claude-switchboard')).toBe(false);
  });
});
