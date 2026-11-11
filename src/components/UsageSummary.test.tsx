import { render, screen, fireEvent, within } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import type { CachedUsage } from '../lib/types';
import { UsageSummary } from './UsageSummary';

function usage(): CachedUsage {
  return {
    snapshot: {
      five_hour: { utilization: 19, resets_at: new Date(Date.now() + 5400_000).toISOString() },
      seven_day: { utilization: 32, resets_at: new Date(Date.now() + 93600_000).toISOString() },
      seven_day_sonnet: { utilization: 20, resets_at: null },
      seven_day_opus: { utilization: 40, resets_at: null },
      seven_day_fable: null,
      extra_usage: null,
      fetched_at: new Date().toISOString(),
    },
    account_id: 'uuid-1',
    account_email: 'a@x.com',
    last_error: null,
    burn_rate: null,
    auth_source: 'OAuth',
  } as CachedUsage;
}

/** A Pro / Team-Pro plan snapshot: the aggregate 7-day bucket is populated,
 * but the API returns null for the per-model opus/sonnet split (only Max
 * plans carry separate per-model weekly quotas). */
function proUsage(): CachedUsage {
  const u = usage();
  u.snapshot.seven_day_sonnet = null;
  u.snapshot.seven_day_opus = null;
  return u;
}

/** A snapshot whose account also has the Fable weekly cap, on its own reset. */
function fableUsage(): CachedUsage {
  const u = usage();
  u.snapshot.seven_day_fable = {
    utilization: 44,
    resets_at: new Date(Date.now() + 4 * 86_400_000 + 7_200_000).toISOString(),
  };
  return u;
}

describe('UsageSummary collapsible details', () => {
  it('explains unsupported token scopes instead of showing misleading usage', () => {
    render(<UsageSummary usage={{ ...usage(), last_error: 'usage_not_supported' }} thresholds={[75, 90]} />);
    expect(screen.getByRole('status')).toHaveTextContent('Usage tracking is unavailable');
    expect(screen.queryByText('19')).toBeNull();
    expect(screen.queryByRole('button', { name: /details/i })).toBeNull();
  });

  it('says it is waiting for Claude Code while the status line bridge has not delivered a reading', () => {
    render(<UsageSummary usage={{ ...usage(), last_error: 'awaiting_statusline' }} thresholds={[75, 90]} />);
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('Waiting for Claude Code');
    expect(status).not.toHaveTextContent('unavailable');
    expect(screen.queryByText('19')).toBeNull();
  });

  it('renders only the hero numbers plus a disclosure row when collapsed', () => {
    const onToggle = vi.fn();
    render(
      <UsageSummary
        usage={usage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen={false}
        onToggleDetails={onToggle}
      />,
    );
    // Hero numbers visible…
    expect(screen.getByText('19')).toBeTruthy();
    expect(screen.getByText('32')).toBeTruthy();
    // …disclosure row present…
    expect(screen.getByRole('button', { name: /details/i })).toBeTruthy();
    // …but the model split is hidden.
    expect(screen.queryByText('Opus')).toBeNull();
    expect(screen.queryByText('Sonnet')).toBeNull();
  });

  it('renders the detail rows when expanded', () => {
    render(
      <UsageSummary
        usage={usage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen
        onToggleDetails={() => {}}
      />,
    );
    expect(screen.getByText('Opus')).toBeTruthy();
    expect(screen.getByText('Sonnet')).toBeTruthy();
  });

  it('shows Fable beside 5h and 7d in the glance view when the account reports one', () => {
    render(
      <UsageSummary
        usage={fableUsage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen={false}
        onToggleDetails={() => {}}
      />,
    );
    const hero = screen.getByTestId('usage-hero');
    expect(within(hero).getByText('5h')).toBeTruthy();
    expect(within(hero).getByText('7d')).toBeTruthy();
    expect(within(hero).getByText('Fable')).toBeTruthy();
    expect(within(hero).getByText('44')).toBeTruthy();
    expect(hero.style.gridTemplateColumns).toBe('repeat(3, minmax(0, 1fr))');
  });

  it('keeps three columns on the smaller hero type with the details open, so 100% fits', () => {
    render(<UsageSummary usage={fableUsage()} thresholds={[75, 90]} collapsible detailsOpen />);
    expect(screen.getByTestId('usage-hero').classList.contains('hero-dense')).toBe(true);
  });

  it('keeps two columns on the full hero type', () => {
    const u = fableUsage();
    u.snapshot.five_hour = null;
    render(<UsageSummary usage={u} thresholds={[75, 90]} plan="ultra" collapsible detailsOpen />);
    expect(screen.getByTestId('usage-hero').classList.contains('hero-dense')).toBe(false);
  });

  it('adds no Fable column when the account reports none', () => {
    render(
      <UsageSummary
        usage={usage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen={false}
        onToggleDetails={() => {}}
      />,
    );
    expect(screen.queryByText('Fable')).toBeNull();
    expect(screen.getByTestId('usage-hero').style.gridTemplateColumns).toBe('repeat(2, minmax(0, 1fr))');
  });

  it('pairs 7d with Fable for an Ultra account that has no 5-hour window', () => {
    const u = fableUsage();
    u.snapshot.five_hour = null;
    render(<UsageSummary usage={u} thresholds={[75, 90]} plan="ultra" collapsible detailsOpen={false} />);
    const hero = screen.getByTestId('usage-hero');
    expect(within(hero).queryByText('5h')).toBeNull();
    expect(within(hero).getByText('7d')).toBeTruthy();
    expect(within(hero).getByText('Fable')).toBeTruthy();
    expect(hero.style.gridTemplateColumns).toBe('repeat(2, minmax(0, 1fr))');
  });

  it('shows Fable once, in the hero, when the details are open', () => {
    render(
      <UsageSummary
        usage={fableUsage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen
        onToggleDetails={() => {}}
      />,
    );
    expect(screen.getAllByText('Fable')).toHaveLength(1);
    expect(within(screen.getByTestId('usage-hero')).getByText('44')).toBeTruthy();
    // The Opus and Sonnet split still sits behind the disclosure.
    expect(screen.getByText('Opus')).toBeTruthy();
  });

  it("gives Fable its own countdown when it resets apart from the weekly window", () => {
    render(<UsageSummary usage={fableUsage()} thresholds={[75, 90]} collapsible detailsOpen={false} />);
    expect(screen.getByText('Fable').parentElement!.textContent).toMatch(/in 4d \d+h/);
  });

  it('drops the Fable countdown when it resets with the weekly window', () => {
    const u = fableUsage();
    const weekly = Date.parse(u.snapshot.seven_day!.resets_at!);
    // The API reports one shared window a few microseconds apart.
    u.snapshot.seven_day_fable = { utilization: 44, resets_at: new Date(weekly + 1).toISOString() };
    render(<UsageSummary usage={u} thresholds={[75, 90]} collapsible detailsOpen={false} />);
    expect(screen.getByText('Fable').parentElement!.textContent).not.toMatch(/in /);
    // The weekly countdown is still there, once.
    expect(screen.getAllByText(/^1d [12]h$/)).toHaveLength(1);
  });

  it('marks Fable as the model in use', () => {
    render(
      <UsageSummary usage={fableUsage()} thresholds={[75, 90]} collapsible detailsOpen={false} activeModel="fable" />,
    );
    const column = screen.getByText('Fable').parentElement!;
    expect(within(column).getByLabelText('currently in use')).toBeTruthy();
  });

  it('marks no hero column when another model is in use', () => {
    render(
      <UsageSummary usage={fableUsage()} thresholds={[75, 90]} collapsible detailsOpen={false} activeModel="opus" />,
    );
    expect(within(screen.getByTestId('usage-hero')).queryByLabelText('currently in use')).toBeNull();
  });

  it('offers no Details disclosure when nothing sits behind it', () => {
    // A Pro account with the Fable cap: every limit it has is in the hero.
    const u = proUsage();
    u.snapshot.seven_day_fable = { utilization: 44, resets_at: null };
    render(<UsageSummary usage={u} thresholds={[75, 90]} collapsible detailsOpen={false} />);
    expect(screen.getByText('Fable')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /details/i })).toBeNull();
  });

  it('keeps the disclosure when pay-as-you-go is the only detail', () => {
    const u = proUsage();
    u.snapshot.extra_usage = { is_enabled: true, utilization: 12, resets_at: null };
    render(<UsageSummary usage={u} thresholds={[75, 90]} collapsible detailsOpen={false} />);
    expect(screen.getByRole('button', { name: /details/i })).toBeTruthy();
    expect(screen.queryByText('Pay-as-you-go')).toBeNull();
  });

  it('clicking the disclosure row toggles', () => {
    const onToggle = vi.fn();
    render(
      <UsageSummary
        usage={usage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen={false}
        onToggleDetails={onToggle}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /details/i }));
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it('renders fully with no disclosure when not collapsible (expanded report)', () => {
    render(
      <UsageSummary usage={usage()} thresholds={[75, 90]} condensed />,
    );
    expect(screen.queryByRole('button', { name: /details/i })).toBeNull();
    expect(screen.getByText('Opus')).toBeTruthy();
  });

  it('hides the Opus/Sonnet rows when the plan provides no per-model data (Pro)', () => {
    // seven_day is present but both per-model buckets are null — the API only
    // breaks out per-model utilization for Max plans. The empty rows must not
    // render, even with details expanded.
    render(
      <UsageSummary
        usage={proUsage()}
        thresholds={[75, 90]}
        collapsible
        detailsOpen
        onToggleDetails={() => {}}
      />,
    );
    expect(screen.queryByText('Opus')).toBeNull();
    expect(screen.queryByText('Sonnet')).toBeNull();
  });

  it('shows only the weekly window for Ultra accounts with no 5-hour limit', () => {
    const u = usage();
    u.snapshot.five_hour = null;
    render(<UsageSummary usage={u} thresholds={[75, 90]} plan="ultra" />);
    expect(screen.queryByText('5h')).toBeNull();
    expect(screen.getByText('7d')).toBeTruthy();
    expect(screen.getByText('32')).toBeTruthy();
  });

  it('keeps the 5-hour window for an unverified Ultra account that still reports one', () => {
    render(<UsageSummary usage={usage()} thresholds={[75, 90]} plan="ultra" />);
    expect(screen.getByText('5h')).toBeTruthy();
    expect(screen.getByText('19')).toBeTruthy();
  });

  it('merges the reset countdown into the bucket label row (no separate caption row)', () => {
    render(<UsageSummary usage={usage()} thresholds={[75, 90]} />);
    // The 5h label row carries its reset time — and nothing else (in the old
    // layout the label's parent was the whole column, hero number included).
    const labelRow = screen.getByText('5h').parentElement!;
    expect(labelRow.textContent).toMatch(/in 1h \d+m/);
    expect(labelRow.textContent).not.toContain('19');
    // Exactly one reset countdown per bucket — not duplicated below the meter.
    expect(screen.getAllByText(/^1h \d+m$/)).toHaveLength(1);
    expect(screen.getAllByText(/^1d [12]h$/)).toHaveLength(1);
  });

  it('keeps the burn-rate projection as the sole bottom caption when present', () => {
    const u = usage();
    u.burn_rate = { utilization_per_min: 0.5, projected_at_reset: 113 };
    render(<UsageSummary usage={u} thresholds={[75, 90]} />);
    expect(screen.getByText(/~113% by reset/)).toBeTruthy();
  });
});
