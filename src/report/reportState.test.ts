import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_SESSIONS_VIEW,
  RANGE_DAYS,
  rehydrateReportStore,
  useReportStore,
} from './reportState';

const KEY = 'report-view-v1';

beforeEach(() => {
  localStorage.clear();
  rehydrateReportStore();
});
afterEach(() => vi.restoreAllMocks());

describe('useReportStore', () => {
  it('starts on the defaults', () => {
    const s = useReportStore.getState();
    expect(s.activeTab).toBe('browse');
    expect(s.range).toBe('30d');
    expect(s.metric).toBe('cost');
    expect(s.sidebarCollapsed).toBe(false);
    expect(s.sessions).toEqual(DEFAULT_SESSIONS_VIEW);
    expect(s.costOpenRow).toBeNull();
  });

  it('maps every range to a number of days', () => {
    expect(RANGE_DAYS).toEqual({ '7d': 7, '30d': 30, '90d': 90, '1y': 365 });
  });

  it('remembers tab, range and measure across a reload', () => {
    useReportStore.getState().setActiveTab('trends');
    useReportStore.getState().setRange('90d');
    useReportStore.getState().setMetric('tokens');
    useReportStore.getState().setSidebarCollapsed(true);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toMatchObject({
      activeTab: 'trends',
      range: '90d',
      metric: 'tokens',
      sidebarCollapsed: true,
    });

    // A reload: the in-memory state is gone, only storage is left.
    useReportStore.setState({ activeTab: 'browse', range: '30d', metric: 'cost', sidebarCollapsed: false });
    rehydrateReportStore();
    expect(useReportStore.getState()).toMatchObject({
      activeTab: 'trends',
      range: '90d',
      metric: 'tokens',
      sidebarCollapsed: true,
    });
  });

  it('does not keep filters, the open row or scroll positions across a reload', () => {
    useReportStore.getState().patchSessions({ query: 'webhook' });
    useReportStore.getState().setCostOpenRow('row-1');
    useReportStore.getState().setScrollTop('cost', 240);
    const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    expect(saved.sessions).toBeUndefined();
    expect(saved.costOpenRow).toBeUndefined();
    expect(saved.scrollTop).toBeUndefined();
  });

  it('works when storage throws', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(() => useReportStore.getState().setActiveTab('cost')).not.toThrow();
    expect(useReportStore.getState().activeTab).toBe('cost');
    expect(() => rehydrateReportStore()).not.toThrow();
    expect(useReportStore.getState().activeTab).toBe('browse');
  });

  it('ignores a corrupt saved value', () => {
    localStorage.setItem(KEY, 'not json');
    rehydrateReportStore();
    expect(useReportStore.getState().activeTab).toBe('browse');
    expect(useReportStore.getState().range).toBe('30d');
  });

  it('ignores an unknown range, measure or wrongly typed field', () => {
    localStorage.setItem(
      KEY,
      JSON.stringify({ activeTab: 42, range: '5y', metric: 'joules', sidebarCollapsed: 'yes' }),
    );
    rehydrateReportStore();
    expect(useReportStore.getState()).toMatchObject({
      activeTab: 'browse',
      range: '30d',
      metric: 'cost',
      sidebarCollapsed: false,
    });
  });

  it('patches the sessions view without dropping the other filters', () => {
    useReportStore.getState().patchSessions({ query: 'webhook' });
    useReportStore.getState().patchSessions({ hasRecapOnly: true });
    expect(useReportStore.getState().sessions).toEqual({
      ...DEFAULT_SESSIONS_VIEW,
      query: 'webhook',
      hasRecapOnly: true,
    });
  });

  it('keeps a scroll position per tab', () => {
    useReportStore.getState().setScrollTop('cost', 240);
    useReportStore.getState().setScrollTop('trends', 12);
    expect(useReportStore.getState().scrollTop).toEqual({ cost: 240, trends: 12 });
  });
});
