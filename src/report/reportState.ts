/**
 * What the expanded window is showing: which tab, over what range, in what
 * measure, with which filters, which row open and how far it is scrolled.
 *
 * It lives outside the components because every tab unmounts when you leave
 * it, and the whole window unmounts when you collapse to the popover. State
 * kept in a component is lost each time; state kept here is still there when
 * you come back.
 *
 * Two lifetimes:
 * - tab, range, measure and the sidebar choice are preferences, saved to
 *   `localStorage` so they survive a restart;
 * - filters, the open row and scroll positions describe one sitting, so they
 *   last while the app runs and start fresh after a restart.
 */
import { create } from 'zustand';

export type ReportRange = '7d' | '30d' | '90d' | '1y';
export type ReportMetric = 'cost' | 'tokens';

export const RANGE_DAYS: Record<ReportRange, number> = { '7d': 7, '30d': 30, '90d': 90, '1y': 365 };

/** How a range is named beside a figure it was worked out for. */
export const RANGE_LABEL: Record<ReportRange, string> = {
  '7d': 'Last 7 days',
  '30d': 'Last 30 days',
  '90d': 'Last 90 days',
  '1y': 'Last 365 days',
};

export type SortOption = 'recent' | 'oldest' | 'turns' | 'tokens' | 'cost' | 'peak_context' | 'title';
export type TimeFilter = 'all' | 'today' | '7d' | '30d';

/** The Sessions tab's search, sort, filters and open row. */
export interface SessionsView {
  query: string;
  sortOption: SortOption;
  projectFilter: string;
  modelFilter: string;
  timeFilter: TimeFilter;
  hasRecapOnly: boolean;
  resumableOnly: boolean;
  groupByProject: boolean;
  expanded: string | null;
}

export const DEFAULT_SESSIONS_VIEW: SessionsView = {
  query: '',
  sortOption: 'recent',
  projectFilter: 'all',
  modelFilter: 'all',
  timeFilter: 'all',
  hasRecapOnly: false,
  resumableOnly: false,
  groupByProject: true,
  expanded: null,
};

interface SavedView {
  activeTab: string;
  range: ReportRange;
  metric: ReportMetric;
  sidebarCollapsed: boolean;
}

interface ReportState extends SavedView {
  scrollTop: Record<string, number>;
  sessions: SessionsView;
  costOpenRow: string | null;
  setActiveTab(id: string): void;
  setRange(range: ReportRange): void;
  setMetric(metric: ReportMetric): void;
  setSidebarCollapsed(collapsed: boolean): void;
  setScrollTop(tab: string, y: number): void;
  patchSessions(patch: Partial<SessionsView>): void;
  setCostOpenRow(id: string | null): void;
}

const STORAGE_KEY = 'report-view-v1';

/** Tabs an older build saved under an id this build no longer has, and the
 * tab each one's content now lives in. Applied when the saved tab is read, so
 * an upgrade reopens on the same content instead of falling back to the first
 * tab. */
export const LEGACY_TAB: Record<string, string> = {
  browse: 'sessions',
  models: 'breakdown',
  projects: 'breakdown',
  cache: 'breakdown',
  heatmap: 'trends',
};

const DEFAULT_SAVED: SavedView = {
  activeTab: 'sessions',
  range: '30d',
  metric: 'cost',
  sidebarCollapsed: false,
};

/** The saved preferences, field by field: anything missing, mistyped or no
 * longer valid falls back to its default instead of poisoning the rest.
 * Storage can throw (blocked, private mode), which also means "nothing saved". */
function readSaved(): SavedView {
  let raw: unknown = null;
  try {
    raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
  } catch {
    raw = null;
  }
  const saved = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  return {
    activeTab:
      typeof saved.activeTab !== 'string'
        ? DEFAULT_SAVED.activeTab
        : Object.hasOwn(LEGACY_TAB, saved.activeTab)
          ? LEGACY_TAB[saved.activeTab]
          : saved.activeTab,
    range:
      typeof saved.range === 'string' && saved.range in RANGE_DAYS
        ? (saved.range as ReportRange)
        : DEFAULT_SAVED.range,
    metric: saved.metric === 'cost' || saved.metric === 'tokens' ? saved.metric : DEFAULT_SAVED.metric,
    sidebarCollapsed:
      typeof saved.sidebarCollapsed === 'boolean' ? saved.sidebarCollapsed : DEFAULT_SAVED.sidebarCollapsed,
  };
}

function writeSaved(state: SavedView) {
  const { activeTab, range, metric, sidebarCollapsed } = state;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ activeTab, range, metric, sidebarCollapsed }));
  } catch {
    // Not remembered this time. The window keeps working.
  }
}

export const useReportStore = create<ReportState>((set, get) => {
  /** Apply a preference change and save it. */
  const remember = (patch: Partial<SavedView>) => {
    set(patch);
    writeSaved(get());
  };
  return {
    ...readSaved(),
    scrollTop: {},
    sessions: DEFAULT_SESSIONS_VIEW,
    costOpenRow: null,
    setActiveTab: (activeTab) => remember({ activeTab }),
    setRange: (range) => remember({ range }),
    setMetric: (metric) => remember({ metric }),
    setSidebarCollapsed: (sidebarCollapsed) => remember({ sidebarCollapsed }),
    setScrollTop: (tab, y) => set((s) => ({ scrollTop: { ...s.scrollTop, [tab]: y } })),
    patchSessions: (patch) => set((s) => ({ sessions: { ...s.sessions, ...patch } })),
    setCostOpenRow: (costOpenRow) => set({ costOpenRow }),
  };
});

/** Put the store back to what a fresh start would give: saved preferences,
 * everything else at its default. Tests call this between cases. */
export function rehydrateReportStore() {
  useReportStore.setState({
    ...readSaved(),
    scrollTop: {},
    sessions: DEFAULT_SESSIONS_VIEW,
    costOpenRow: null,
  });
}
