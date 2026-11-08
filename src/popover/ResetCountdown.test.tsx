import { render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ResetCountdown } from './ResetCountdown';

const NOW = new Date('2026-11-09T12:00:00Z').getTime();

function at(msFromNow: number): string {
  return new Date(NOW + msFromNow).toISOString();
}

describe('ResetCountdown', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('shows days and hours once more than a day remains', () => {
    render(<ResetCountdown resetsAt={at(((4 * 24 + 10) * 60 + 12) * 60_000)} />);
    expect(screen.getByText('4d 10h')).toBeTruthy();
  });

  it('shows days and hours at exactly 26 hours', () => {
    render(<ResetCountdown resetsAt={at(26 * 60 * 60_000)} />);
    expect(screen.getByText('1d 2h')).toBeTruthy();
  });

  it('keeps hours and minutes below a day', () => {
    render(<ResetCountdown resetsAt={at((2 * 60 + 15) * 60_000)} />);
    expect(screen.getByText('2h 15m')).toBeTruthy();
  });

  it('keeps minutes alone below an hour', () => {
    render(<ResetCountdown resetsAt={at(45 * 60_000)} />);
    expect(screen.getByText('45m')).toBeTruthy();
  });
});