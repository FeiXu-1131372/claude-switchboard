import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import type { AccountListEntry, CachedUsage } from '../../lib/generated/bindings';

const ipcMock = vi.hoisted(() => ({
  getWarmupState: vi.fn().mockResolvedValue({ warmup_enabled: false, schedule: { type: 'Off' } }),
  getWarmupConsentGranted: vi.fn().mockResolvedValue(true),
  setWarmupEnabled: vi.fn().mockResolvedValue(undefined),
  setAccountSchedule: vi.fn().mockResolvedValue(undefined),
  warmupAccountNow: vi.fn().mockResolvedValue('Success'),
  grantWarmupConsent: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('../../lib/ipc', () => ({ ipc: ipcMock }));

import { AccountRow } from '../AccountRow';
import { sameReset } from '../../lib/reset';

function cachedUsage(
  util5h: number | null,
  util7d: number | null,
  utilFable: number | null = null,
): CachedUsage {
  return {
    snapshot: {
      five_hour:
        util5h === null
          ? null
          : { utilization: util5h, resets_at: new Date(Date.now() + 3600_000).toISOString() },
      seven_day:
        util7d === null
          ? null
          : { utilization: util7d, resets_at: new Date(Date.now() + 86400_000).toISOString() },
      seven_day_sonnet: null,
      seven_day_opus: null,
      seven_day_fable:
        utilFable === null
          ? null
          : { utilization: utilFable, resets_at: new Date(Date.now() + 86400_000).toISOString() },
      extra_usage: null,
      fetched_at: new Date(Date.now() - 3 * 60_000).toISOString(),
    },
    account_id: 'uuid-1',
    account_email: 'a@x.com',
    last_error: 'rate-limited (429)',
    burn_rate: null,
    auth_source: 'OAuth',
  } as CachedUsage;
}

function entry(overrides: Partial<AccountListEntry>): AccountListEntry {
  return {
    slot: 1,
    email: 'a@x.com',
    account_uuid: 'uuid-1',
    org_name: null,
    org_uuid: null,
    subscription_type: 'pro',
    source: 'OAuth',
    is_active: true,
    cached_usage: null,
    last_error: null,
    ...overrides,
  } as AccountListEntry;
}

describe('AccountRow error rendering', () => {
  it('explains unsupported usage without requesting browser reauthentication', () => {
    render(<AccountRow entry={entry({ source: 'LongLivedToken', last_error: 'usage_not_supported' })} thresholds={[75, 90]} onReauth={vi.fn()} />);
    expect(screen.getByText('Usage tracking unavailable for this token')).toBeInTheDocument();
    expect(screen.queryByText(/token expired/i)).toBeNull();
    expect(screen.queryByRole('button', { name: /re-authenticate/i })).toBeNull();
  });

  it('shows a muted waiting line, not a warning, while the bridge has no reading yet', () => {
    render(<AccountRow entry={entry({ source: 'LongLivedToken', last_error: 'awaiting_statusline' })} thresholds={[75, 90]} onReauth={vi.fn()} />);
    const line = screen.getByText('Waiting for Claude Code activity');
    expect(line).toBeInTheDocument();
    expect(line.className).toContain('--color-text-muted');
    expect(line.className).not.toContain('--color-warn');
    expect(screen.queryByText(/unavailable/i)).toBeNull();
    expect(screen.queryByRole('button', { name: /re-authenticate|replace token$/i })).toBeNull();
  });

  it('offers token replacement in the account menu', () => {
    const replace = vi.fn();
    render(<AccountRow entry={entry({ source: 'LongLivedToken', is_active: false })} thresholds={[75, 90]} onReauth={replace} onRemove={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'More actions' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Replace token…' }));
    expect(replace).toHaveBeenCalledOnce();
  });

  it('hides the 5-hour meter for Ultra accounts with no 5-hour limit', () => {
    render(
      <AccountRow
        entry={entry({ subscription_type: 'ultra', cached_usage: cachedUsage(null, 63) })}
        thresholds={[75, 90]}
      />,
    );
    expect(screen.queryByText('5h')).toBeNull();
    expect(screen.getByText('7d')).toBeTruthy();
    expect(screen.getByText('63%')).toBeTruthy();
  });

  it('shows the Fable meter when the snapshot carries one', () => {
    render(
      <AccountRow
        entry={entry({ subscription_type: 'ultra', cached_usage: cachedUsage(null, 63, 44) })}
        thresholds={[75, 90]}
      />,
    );
    expect(screen.getByText('Fable')).toBeTruthy();
    expect(screen.getByText('44%')).toBeTruthy();
  });

  it('shows last-good meters with a stale hint on transient errors', () => {
    render(
      <AccountRow
        entry={entry({ last_error: 'rate-limited (429)', cached_usage: cachedUsage(42, 63) })}
        thresholds={[75, 90]}
      />,
    );
    // Last-known-good data stays visible instead of being hidden by the error.
    expect(screen.getByText('42%')).toBeTruthy();
    expect(screen.getByText('63%')).toBeTruthy();
    expect(screen.queryByText('usage unavailable')).toBeNull();
    // …and a hint explains the data is stale.
    expect(screen.getByText(/rate-limited/i)).toBeTruthy();
    expect(screen.getByText(/ago/)).toBeTruthy();
  });

  it('shows "usage unavailable" only when there is no usable snapshot', () => {
    render(
      <AccountRow
        entry={entry({ last_error: 'rate-limited (429)', cached_usage: cachedUsage(null, null) })}
        thresholds={[75, 90]}
      />,
    );
    expect(screen.getByText('usage unavailable')).toBeTruthy();
  });

  it('auth_required keeps the re-authenticate action even when data exists', () => {
    const cached = { ...cachedUsage(42, 63), last_error: 'auth_required' };
    render(
      <AccountRow
        entry={entry({ last_error: 'auth_required', cached_usage: cached })}
        thresholds={[75, 90]}
        onReauth={() => {}}
      />,
    );
    expect(screen.getByText(/token expired/i)).toBeTruthy();
    expect(screen.getByRole('button', { name: /re-authenticate/i })).toBeTruthy();
    expect(screen.queryByText('42%')).toBeNull();
  });

  it('renders meters normally when there is no error', () => {
    render(
      <AccountRow
        entry={entry({ last_error: null, cached_usage: cachedUsage(42, 63) })}
        thresholds={[75, 90]}
      />,
    );
    expect(screen.getByText('42%')).toBeTruthy();
    expect(screen.queryByText(/rate-limited/i)).toBeNull();
  });
});

describe('AccountRow warm-up section', () => {
  it('is collapsed by default — controls hidden, disclosure line visible', () => {
    render(<AccountRow entry={entry({})} thresholds={[75, 90]} />);
    expect(screen.getByRole('button', { name: /warm-up/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: /warm up now/i })).toBeNull();
    expect(screen.queryByRole('switch')).toBeNull();
  });

  it('expands on click to reveal the warm-up controls', async () => {
    ipcMock.getWarmupState.mockResolvedValueOnce({
      warmup_enabled: true,
      schedule: { type: 'Every5h', anchor: { hour: 6, minute: 0 } },
    });
    render(<AccountRow entry={entry({})} thresholds={[75, 90]} />);
    fireEvent.click(screen.getByRole('button', { name: /warm-up/i }));
    expect(screen.getByRole('switch')).toBeTruthy();
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /warm up now/i })).toBeTruthy(),
    );
  });

  it('collapses again on a second click', async () => {
    render(<AccountRow entry={entry({})} thresholds={[75, 90]} />);
    const disclosure = screen.getByRole('button', { name: /warm-up/i });
    fireEvent.click(disclosure);
    expect(screen.getByRole('switch')).toBeTruthy();
    fireEvent.click(disclosure);
    expect(screen.queryByRole('switch')).toBeNull();
  });

  // An account at 0% of its 5-hour window got a bare HTTP 429 from the API. The
  // row used to call that "Already at rate limit", which was untrue.
  it('says a refusal was a refusal, with its request id, not a rate limit', async () => {
    ipcMock.getWarmupState.mockResolvedValueOnce({ warmup_enabled: true, schedule: { type: 'Off' } });
    ipcMock.warmupAccountNow.mockResolvedValueOnce({
      Refused: { detail: 'HTTP 429, not a usage limit; request req_abc' },
    });
    render(<AccountRow entry={entry({})} thresholds={[75, 90]} />);
    fireEvent.click(await screen.findByRole('button', { name: /warm-up/i }));
    fireEvent.click(await screen.findByRole('button', { name: /warm up now/i }));
    expect(await screen.findByText(/refused the request.*req_abc/i)).toBeTruthy();
    expect(screen.queryByText(/already at rate limit/i)).toBeNull();
  });

  it('still calls a genuine usage limit a rate limit', async () => {
    ipcMock.getWarmupState.mockResolvedValueOnce({ warmup_enabled: true, schedule: { type: 'Off' } });
    ipcMock.warmupAccountNow.mockResolvedValueOnce('AtRateLimit');
    render(<AccountRow entry={entry({})} thresholds={[75, 90]} />);
    fireEvent.click(await screen.findByRole('button', { name: /warm-up/i }));
    fireEvent.click(await screen.findByRole('button', { name: /warm up now/i }));
    expect(await screen.findByText(/already at rate limit/i)).toBeTruthy();
  });

  it('summarizes the schedule on the collapsed line', async () => {
    ipcMock.getWarmupState.mockResolvedValueOnce({
      warmup_enabled: true,
      schedule: { type: 'Every5h', anchor: { hour: 6, minute: 0 } },
    });
    render(<AccountRow entry={entry({})} thresholds={[75, 90]} />);
    expect(await screen.findByText('Every 5h')).toBeTruthy();
    // Summary lives on the collapsed line — controls stay hidden.
    expect(screen.queryByRole('switch')).toBeNull();
  });
});

/** An Ultra snapshot whose weekly and Fable windows reset `fableHoursLater`
 * hours apart. The API stamps the two a few microseconds apart even when they
 * are the same window, as here. */
function ultraUsage(fableHoursLater: number): CachedUsage {
  const usage = cachedUsage(null, 63, 44);
  usage.snapshot.seven_day!.resets_at = '2099-11-14T02:59:59.570752Z';
  usage.snapshot.seven_day_fable!.resets_at = new Date(
    Date.parse('2099-11-14T02:59:59.570874Z') + fableHoursLater * 3_600_000,
  ).toISOString();
  return usage;
}

/** Each countdown reads "in <time>"; its own text is the word "in". */
const countdowns = () => screen.queryAllByText('in');

describe('sameReset', () => {
  it('is true for two stamps of the same moment', () => {
    expect(sameReset('2026-11-14T02:59:59.570752Z', '2026-11-14T02:59:59.570874Z')).toBe(true);
    expect(sameReset('2026-11-14T02:59:00Z', '2026-11-14T03:00:00Z')).toBe(true);
  });

  it('is false for different moments', () => {
    expect(sameReset('2026-11-14T02:59:59Z', '2026-11-14T05:00:00Z')).toBe(false);
    expect(sameReset('2026-11-14T02:59:00Z', '2026-11-14T03:00:01Z')).toBe(false);
  });

  it('is false when either is missing or unreadable', () => {
    expect(sameReset(null, '2026-11-14T05:00:00Z')).toBe(false);
    expect(sameReset('2026-11-14T05:00:00Z', undefined)).toBe(false);
    expect(sameReset(null, null)).toBe(false);
    expect(sameReset('soon', 'soon')).toBe(false);
  });
});

describe('AccountRow reset times', () => {
  it('shows one reset time when the weekly and Fable windows reset together', () => {
    render(
      <AccountRow
        entry={entry({ subscription_type: 'ultra', cached_usage: ultraUsage(0) })}
        thresholds={[75, 90]}
      />,
    );
    expect(screen.getByText('Fable')).toBeInTheDocument();
    expect(countdowns()).toHaveLength(1);
  });

  it('shows both reset times when they differ', () => {
    render(
      <AccountRow
        entry={entry({ subscription_type: 'ultra', cached_usage: ultraUsage(2) })}
        thresholds={[75, 90]}
      />,
    );
    expect(countdowns()).toHaveLength(2);
  });

  it('keeps the Fable reset time when the weekly window has none', () => {
    const usage = ultraUsage(0);
    usage.snapshot.seven_day!.resets_at = null;
    render(
      <AccountRow entry={entry({ subscription_type: 'ultra', cached_usage: usage })} thresholds={[75, 90]} />,
    );
    expect(countdowns()).toHaveLength(1);
  });
});

describe('AccountRow switch', () => {
  it('Switch is visible without hovering', () => {
    render(<AccountRow entry={entry({ is_active: false })} onSwap={vi.fn()} thresholds={[75, 90]} />);
    const btn = screen.getByRole('button', { name: 'Switch to a@x.com' });
    expect(btn.className).not.toMatch(/opacity-0/);
    expect(btn).toHaveTextContent('Switch');
  });

  it('asks to switch when pressed', () => {
    const onSwap = vi.fn();
    render(<AccountRow entry={entry({ is_active: false })} onSwap={onSwap} thresholds={[75, 90]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Switch to a@x.com' }));
    expect(onSwap).toHaveBeenCalledTimes(1);
  });

  it('says so while it is switching, and cannot be pressed again', () => {
    const onSwap = vi.fn();
    render(
      <AccountRow entry={entry({ is_active: false })} onSwap={onSwap} swapBusy swapping thresholds={[75, 90]} />,
    );
    const btn = screen.getByRole('button', { name: 'Switching to a@x.com' });
    expect(btn).toHaveTextContent('Switching…');
    expect(btn).toBeDisabled();
    fireEvent.click(btn);
    expect(onSwap).not.toHaveBeenCalled();
  });

  it('waits while another account is switching', () => {
    render(<AccountRow entry={entry({ is_active: false })} onSwap={vi.fn()} swapBusy thresholds={[75, 90]} />);
    const btn = screen.getByRole('button', { name: 'Switch to a@x.com' });
    expect(btn).toBeDisabled();
    expect(btn).toHaveTextContent('Switch');
  });

  it('the active account has nothing to switch to', () => {
    render(<AccountRow entry={entry({ is_active: true })} onSwap={vi.fn()} thresholds={[75, 90]} />);
    expect(screen.queryByRole('button', { name: /^Switch/ })).toBeNull();
    expect(screen.getByText('Active')).toBeInTheDocument();
  });
});

describe('AccountRow for an address saved under two organizations', () => {
  it('shows the organization beside the address, so the two rows can be told apart', () => {
    render(
      <AccountRow
        entry={entry({ email: 'eric@x.com', org_name: 'Catch Software' })}
        thresholds={[75, 90]}
        qualifier="Catch Software"
      />,
    );
    expect(screen.getByText('eric@x.com')).toBeInTheDocument();
    expect(screen.getByText('Catch Software')).toBeInTheDocument();
  });

  it('names the organization on the switch button too', () => {
    render(
      <AccountRow
        entry={entry({ email: 'eric@x.com', is_active: false })}
        thresholds={[75, 90]}
        qualifier="Catch Software"
        onSwap={vi.fn()}
      />,
    );
    expect(screen.getByRole('button', { name: 'Switch to eric@x.com (Catch Software)' })).toBeInTheDocument();
  });

  it('adds nothing when the address is not shared', () => {
    render(<AccountRow entry={entry({ email: 'eric@x.com', is_active: false })} thresholds={[75, 90]} onSwap={vi.fn()} />);
    expect(screen.getByRole('button', { name: 'Switch to eric@x.com' })).toBeInTheDocument();
  });
});
