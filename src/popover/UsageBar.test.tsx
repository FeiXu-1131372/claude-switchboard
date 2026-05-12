import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { UsageBar } from './UsageBar';
import type { Utilization } from '../lib/types';

// Precautionary Tauri mock — UsageBar itself has no Tauri imports, but the
// test runner shares a module registry and some transitive imports may touch
// the Tauri bridge.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

function makeUtil(utilization: number): Utilization {
  return { utilization, resets_at: new Date(Date.now() + 3_600_000).toISOString() };
}

describe('UsageBar', () => {
  it('shows accent fill when utilization is below warn threshold', () => {
    const { container } = render(
      <UsageBar label="5h" data={makeUtil(50)} warnAt={75} dangerAt={90} />,
    );
    // The filled bar is the inner div carrying the theme-color class.
    // It sits inside the track (which has overflow-hidden).
    const fill = container.querySelector('[style*="width"]');
    expect(fill?.className).toContain('bg-[var(--color-accent)]');
  });

  it('shows warn fill at warn threshold', () => {
    const { container } = render(
      <UsageBar label="5h" data={makeUtil(75)} warnAt={75} dangerAt={90} />,
    );
    const fill = container.querySelector('[style*="width"]');
    expect(fill?.className).toContain('bg-[var(--color-warn)]');
  });

  it('shows danger fill above danger threshold', () => {
    const { container } = render(
      <UsageBar label="5h" data={makeUtil(95)} warnAt={75} dangerAt={90} />,
    );
    const fill = container.querySelector('[style*="width"]');
    expect(fill?.className).toContain('bg-[var(--color-danger)]');
  });

  it('renders without crashing when data is null', () => {
    // When data is null and no value prop is provided, the component shows the
    // "n/a" placeholder — it must not throw.
    render(<UsageBar label="5h" data={null} />);
    expect(screen.getByText('5h')).toBeInTheDocument();
  });
});
