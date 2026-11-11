import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ReportToolbar } from './ReportToolbar';
import { useReportStore } from './reportState';

describe('ReportToolbar', () => {
  it('shows the headline figure and what it covers', () => {
    render(<ReportToolbar figure="$13,214" caption="30 days" />);
    expect(screen.getByText('$13,214')).toBeInTheDocument();
    expect(screen.getByText('30 days')).toBeInTheDocument();
  });

  it('changes the shared range', () => {
    render(<ReportToolbar figure="$13,214" caption="30 days" />);
    expect(screen.getByRole('button', { name: '30d' })).toHaveAttribute('aria-pressed', 'true');
    fireEvent.click(screen.getByRole('button', { name: '90d' }));
    expect(useReportStore.getState().range).toBe('90d');
    expect(screen.getByRole('button', { name: '90d' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('shows the measure switch only when asked', () => {
    const { rerender } = render(<ReportToolbar figure="x" />);
    expect(screen.queryByRole('group', { name: 'Metric' })).toBeNull();
    rerender(<ReportToolbar figure="x" showMetric />);
    expect(screen.getByRole('button', { name: 'Tokens' })).toBeInTheDocument();
  });

  it('changes the shared measure', () => {
    render(<ReportToolbar figure="x" showMetric />);
    fireEvent.click(screen.getByRole('button', { name: 'Tokens' }));
    expect(useReportStore.getState().metric).toBe('tokens');
  });

  it('can leave the range out', () => {
    render(<ReportToolbar figure="x" showRange={false} />);
    expect(screen.queryByRole('group', { name: 'Range' })).toBeNull();
  });

  it('puts a tab’s own controls before the range', () => {
    render(
      <ReportToolbar figure="x">
        <button type="button">Weekly</button>
      </ReportToolbar>,
    );
    const own = screen.getByRole('button', { name: 'Weekly' });
    const range = screen.getByRole('group', { name: 'Range' });
    expect(own.compareDocumentPosition(range) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('announces the figure when it changes', () => {
    render(<ReportToolbar figure="$1.00" caption="7 days" />);
    expect(screen.getByText('$1.00').closest('[aria-live]')).toHaveAttribute('aria-live', 'polite');
  });
});
