import { describe, expect, it } from 'vitest';
import type { AccountListEntry } from '../../lib/generated/bindings';
import { accountQualifiers } from '../accountQualifiers';

function entry(slot: number, email: string, org_name: string | null): AccountListEntry {
  return {
    slot,
    email,
    account_uuid: `uuid-${slot}`,
    org_name,
    org_uuid: org_name ? `org-${slot}` : null,
    subscription_type: null,
    source: 'OAuth',
    is_active: false,
    cached_usage: null,
    last_error: null,
  } as AccountListEntry;
}

describe('accountQualifiers', () => {
  it('names the organization of each account when one email is saved twice', () => {
    const q = accountQualifiers([
      entry(1, 'eric@example.com', 'Catch Software'),
      entry(2, 'eric@example.com', "eric@example.com's Organization"),
    ]);
    expect(q.get(1)).toBe('Catch Software');
    expect(q.get(2)).toBe("eric@example.com's Organization");
  });

  it('says nothing about an address that appears once', () => {
    const q = accountQualifiers([
      entry(1, 'eric@example.com', 'Catch Software'),
      entry(2, 'sam@example.com', 'Sam Org'),
    ]);
    expect(q.size).toBe(0);
  });

  it('treats the same address in another letter case as the same address', () => {
    const q = accountQualifiers([
      entry(1, 'Eric@Example.com', 'Team A'),
      entry(2, 'eric@example.com', 'Personal'),
    ]);
    expect(q.get(1)).toBe('Team A');
    expect(q.get(2)).toBe('Personal');
  });

  it('leaves out an account whose organization is not known', () => {
    const q = accountQualifiers([
      entry(1, 'eric@example.com', 'Catch Software'),
      entry(2, 'eric@example.com', null),
    ]);
    expect(q.get(1)).toBe('Catch Software');
    expect(q.has(2)).toBe(false);
  });
});
