import { describe, it, expect } from 'vitest';
import { modelKey, shortName, MODEL_VARIANT } from './modelDisplay';

describe('modelKey', () => {
  it('classifies fable/opus/sonnet/haiku regardless of case or version suffix', () => {
    expect(modelKey('claude-fable-5-1')).toBe('fable');
    expect(modelKey('claude-opus-5-5')).toBe('opus');
    expect(modelKey('claude-opus-4-7')).toBe('opus');
    expect(modelKey('Claude-Sonnet-5-5')).toBe('sonnet');
    expect(modelKey('Claude-Sonnet-4-6')).toBe('sonnet');
    expect(modelKey('claude-haiku-5-5')).toBe('haiku');
  });

  it('falls back to default for unrecognized third-party models', () => {
    expect(modelKey('glm-5.3')).toBe('default');
    expect(modelKey('deepseek-v4.1-flash-1017')).toBe('default');
  });
});

describe('shortName', () => {
  it('extracts a compact "tier version" label including fable and 5.x models', () => {
    expect(shortName('claude-fable-5-1')).toBe('fable 5-1');
    expect(shortName('claude-opus-5-5')).toBe('opus 5-5');
    expect(shortName('claude-sonnet-5-5')).toBe('sonnet 5-5');
    expect(shortName('claude-opus-4-7')).toBe('opus 4-7');
    expect(shortName('claude-sonnet-4-6')).toBe('sonnet 4-6');
    expect(shortName('anthropic/claude-fable-5.1')).toBe('fable 5-1');
  });

  it('returns the cleaned name when no Anthropic tier pattern matches', () => {
    expect(shortName('glm-5.3')).toBe('glm-5.3');
    expect(shortName('deepseek-v4.1-flash-1017')).toBe('deepseek-v4.1-flash-1017');
  });
});

describe('MODEL_VARIANT', () => {
  it('maps known tiers to their badge variant', () => {
    expect(MODEL_VARIANT.fable).toBe('fable');
    expect(MODEL_VARIANT.opus).toBe('opus');
    expect(MODEL_VARIANT.sonnet).toBe('sonnet');
    expect(MODEL_VARIANT.haiku).toBe('haiku');
  });
});
