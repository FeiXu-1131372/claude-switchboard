import type { AccountListEntry } from '../lib/generated/bindings';

/**
 * What tells apart two saved accounts that share an email address.
 *
 * One person can be a Team seat and have a personal login under the same
 * address. They are two accounts, with their own limits and their own tokens,
 * and the address alone cannot say which row is which. Their organizations
 * can, so an address that appears more than once gets its organization's name
 * beside it. An address that appears once is left alone, as is an account
 * whose organization has not been recorded (nothing to show).
 *
 * Keyed by slot.
 */
export function accountQualifiers(accounts: readonly AccountListEntry[]): Map<number, string> {
  const perAddress = new Map<string, number>();
  for (const a of accounts) {
    const address = a.email.toLowerCase();
    perAddress.set(address, (perAddress.get(address) ?? 0) + 1);
  }
  const qualifiers = new Map<number, string>();
  for (const a of accounts) {
    if ((perAddress.get(a.email.toLowerCase()) ?? 0) < 2) continue;
    if (a.org_name) qualifiers.set(a.slot, a.org_name);
  }
  return qualifiers;
}
