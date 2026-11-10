/**
 * Plan-name helpers.
 *
 * The backend normalizes Claude Code's OAuth blob to these display names,
 * including mapping the old $200 "Max 20x" rate-limit tier to "ultra".
 */

/** True for the $200 Ultra plan — verified accounts have no 5-hour window. */
export function isUltraPlan(plan: string | null | undefined): boolean {
  return plan?.trim().toLowerCase() === 'ultra';
}

/** Max and Ultra share the filled/accent plan-badge treatment. */
export function isPremiumPlan(plan: string | null | undefined): boolean {
  const normalized = plan?.trim().toLowerCase();
  return normalized === 'max' || normalized === 'ultra';
}