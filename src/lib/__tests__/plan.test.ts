import { describe, it, expect } from 'vitest';
import { isPremiumPlan, isUltraPlan } from '../plan';

describe('plan helpers', () => {
  it('recognizes Ultra regardless of case or surrounding space', () => {
    expect(isUltraPlan('ultra')).toBe(true);
    expect(isUltraPlan(' Ultra ')).toBe(true);
    expect(isUltraPlan('max')).toBe(false);
    expect(isUltraPlan('pro')).toBe(false);
    expect(isUltraPlan(null)).toBe(false);
    expect(isUltraPlan(undefined)).toBe(false);
  });

  it('treats Max and Ultra as premium, but not Pro', () => {
    expect(isPremiumPlan('max')).toBe(true);
    expect(isPremiumPlan('ULTRA')).toBe(true);
    expect(isPremiumPlan('pro')).toBe(false);
    expect(isPremiumPlan(null)).toBe(false);
  });
});