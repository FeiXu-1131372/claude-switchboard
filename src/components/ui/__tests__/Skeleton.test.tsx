import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ShareBar } from '../ShareBar';
import { SkeletonRows } from '../Skeleton';

describe('SkeletonRows', () => {
  it('announces loading', () => {
    render(<SkeletonRows rows={3} />);
    expect(screen.getByRole('status', { name: 'Loading' })).toBeInTheDocument();
  });

  it('draws the number of rows asked for', () => {
    render(<SkeletonRows rows={3} />);
    expect(screen.getAllByTestId('skeleton-row')).toHaveLength(3);
  });
});

describe('ShareBar', () => {
  const fill = () => screen.getByTestId('share-bar-fill');

  it('fills to the share', () => {
    render(<ShareBar pct={42.5} />);
    expect(fill().style.width).toBe('42.5%');
  });

  it('never overflows or goes negative', () => {
    const { rerender } = render(<ShareBar pct={180} />);
    expect(fill().style.width).toBe('100%');
    rerender(<ShareBar pct={-5} />);
    expect(fill().style.width).toBe('0%');
  });

  it('draws nothing for a share that is not a number', () => {
    render(<ShareBar pct={Number.NaN} />);
    expect(fill().style.width).toBe('0%');
  });

  it('takes a colour', () => {
    render(<ShareBar pct={10} color="var(--color-teal)" />);
    expect(fill().style.background).toBe('var(--color-teal)');
  });
});
