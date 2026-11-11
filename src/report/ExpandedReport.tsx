import { useLayoutEffect, useRef, useState, type KeyboardEvent, type UIEvent } from 'react';
import { motion } from 'framer-motion';
import { Badge } from '../components/ui/Badge';
import { IconButton } from '../components/ui/IconButton';
import { SessionsTab } from './SessionsTab';
import { TrendsTab } from './TrendsTab';
import { BreakdownTab } from './BreakdownTab';
import { useAppStore } from '../lib/store';
import { ipc } from '../lib/ipc';
import { tabSlide } from '../lib/motion';
import { IconRefresh, IconCollapse, IconSettings, IconSidebarShow, X } from '../lib/icons';
import { handleDragStart, closeWindow } from '../lib/window-chrome';
import { AccountsSidebar } from '../accounts/AccountsSidebar';
import { SettingsModal } from '../components/modals/SettingsModal';
import { ProvidersTab } from '../providers/ProvidersTab';
import { SessionsBrowserTab } from '../sessions/SessionsBrowserTab';
import { useReportStore } from './reportState';

const TAB_CONFIG = [
  { id: 'sessions', label: 'Sessions' },
  { id: 'cost', label: 'Cost' },
  { id: 'trends', label: 'Trends' },
  { id: 'breakdown', label: 'Breakdown' },
  { id: 'providers', label: 'Providers' },
] as const;

const TAB_COMPONENTS: Record<string, React.FC> = {
  sessions: SessionsBrowserTab,
  cost: SessionsTab,
  trends: TrendsTab,
  breakdown: BreakdownTab,
  providers: ProvidersTab,
};

const TAB_IDS: readonly string[] = TAB_CONFIG.map((t) => t.id);

export function ExpandedReport() {
  // The tab is remembered across restarts. The store already reads a tab an
  // older build folded into another as that other tab; any other id this
  // build does not have opens the first tab instead of a blank panel.
  const savedTab = useReportStore((s) => s.activeTab);
  const setActiveTab = useReportStore((s) => s.setActiveTab);
  const activeTab = TAB_IDS.includes(savedTab) ? savedTab : TAB_IDS[0];
  const [refreshing, setRefreshing] = useState(false);
  const [tabKey, setTabKey] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const prevTabRef = useRef<string>(activeTab);
  const stale = useAppStore((s) => s.stale);
  const toggleViewMode = useAppStore((s) => s.toggleViewMode);

  /* ── Accounts column ──
   * Hidden, it gives its width to the content. The button that hides it goes
   * away with it, so focus is handed to the one that brings it back (and the
   * other way round) rather than dropped. Only after a press: a window that
   * opens with the column hidden takes no focus for it. */
  const sidebarCollapsed = useReportStore((s) => s.sidebarCollapsed);
  const setSidebarCollapsed = useReportStore((s) => s.setSidebarCollapsed);
  const showSidebarRef = useRef<HTMLButtonElement>(null);
  const hideSidebarRef = useRef<HTMLButtonElement>(null);
  const handFocusOn = useRef(false);

  function setSidebar(collapsed: boolean) {
    handFocusOn.current = true;
    setSidebarCollapsed(collapsed);
  }

  useLayoutEffect(() => {
    if (!handFocusOn.current) return;
    handFocusOn.current = false;
    (sidebarCollapsed ? showSidebarRef : hideSidebarRef).current?.focus();
  }, [sidebarCollapsed]);

  const TabComponent = TAB_COMPONENTS[activeTab] ?? SessionsTab;

  const tabIds = TAB_IDS;
  const prevIdx = tabIds.indexOf(prevTabRef.current);
  const currIdx = tabIds.indexOf(activeTab);
  const slideDir = currIdx >= prevIdx ? 1 : -1;
  prevTabRef.current = activeTab;

  /* ── Scroll position, per tab ──
   * Each tab comes back to where it was left. The position is put back
   * before paint; if the content is not tall enough yet (data still on its
   * way), it is retried as the content grows, until it is reached, the user
   * scrolls for themselves, or two seconds pass. */
  const scrollRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const restoring = useRef<number | null>(null);
  const pendingSave = useRef<{ tab: string; y: number; frame: number } | null>(null);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    const content = contentRef.current;
    if (!el || !content) return;
    const target = useReportStore.getState().scrollTop[activeTab] ?? 0;
    el.scrollTop = target;
    if (el.scrollTop >= target) {
      restoring.current = null;
      return;
    }
    restoring.current = target;
    const observer = new ResizeObserver(() => {
      if (restoring.current === null) return;
      el.scrollTop = restoring.current;
      if (el.scrollTop >= restoring.current) restoring.current = null;
    });
    observer.observe(content);
    const giveUp = window.setTimeout(() => {
      restoring.current = null;
    }, 2000);
    return () => {
      observer.disconnect();
      window.clearTimeout(giveUp);
    };
  }, [activeTab, tabKey]);

  function handleScroll(e: UIEvent<HTMLDivElement>) {
    // While a position is being put back, the scroll events are ours.
    if (restoring.current !== null) return;
    // Tab and position are read now, not in the frame callback: by then a
    // tab switch may have replaced both, and the old tab would be saved at
    // the new tab's position.
    const next = { tab: activeTab, y: e.currentTarget.scrollTop };
    if (pendingSave.current) {
      pendingSave.current.tab = next.tab;
      pendingSave.current.y = next.y;
      return;
    }
    const frame = requestAnimationFrame(() => {
      const save = pendingSave.current;
      pendingSave.current = null;
      if (save) useReportStore.getState().setScrollTop(save.tab, save.y);
    });
    pendingSave.current = { ...next, frame };
  }

  function stopRestoring() {
    restoring.current = null;
  }

  async function handleRefresh() {
    if (refreshing) return;
    setRefreshing(true);
    try {
      await ipc.forceRefresh('active');
      setTabKey((k) => k + 1);
    } finally {
      setTimeout(() => setRefreshing(false), 420);
    }
  }

  return (
    <>
      <div
        className="flex h-full overflow-hidden"
        style={{
          width: '100%',
          minHeight: 'var(--report-min-height)',
          background: 'var(--color-bg-base)',
        }}
      >
        {!sidebarCollapsed && (
          <AccountsSidebar onHide={() => setSidebar(true)} hideButtonRef={hideSidebarRef} />
        )}
        <div className="flex flex-1 flex-col overflow-hidden min-w-0">
          {/* One row: where you are on the left, the window's own controls on
              the right, and between them room to take hold of the window. */}
          <header
            onPointerDown={handleDragStart}
            className="
              flex h-[var(--shell-header-height)] shrink-0 items-stretch gap-[var(--space-md)]
              border-b border-[var(--color-rule)]
              pl-[var(--space-2xl)] pr-[var(--space-md)]
              cursor-default select-none
            "
          >
            {sidebarCollapsed && (
              <IconButton
                ref={showSidebarRef}
                label="Show accounts"
                // Pulled left so its glyph, not its box, lines up with the content.
                className="-ml-[var(--space-sm)] self-center"
                onClick={() => setSidebar(false)}
              >
                <IconSidebarShow size={14} />
              </IconButton>
            )}
            <TabBar
              activeId={activeTab}
              onSelect={setActiveTab}
              tabs={TAB_CONFIG.map((t) => ({ id: t.id, label: t.label }))}
            />
            <div className="flex-1" />
            <div className="flex items-center gap-[var(--space-2xs)]">
              {stale && (
                <Badge
                  variant="warn"
                  role="status"
                  title="The last refresh did not complete, so these figures may be out of date."
                  className="mr-[var(--space-xs)]"
                >
                  Stale
                </Badge>
              )}
              <IconButton label="Refresh" onClick={handleRefresh}>
                <motion.span
                  animate={refreshing ? { rotate: 360 } : { rotate: 0 }}
                  transition={
                    refreshing
                      ? { duration: 0.7, ease: 'linear', repeat: Infinity }
                      : { duration: 0.2 }
                  }
                  style={{ display: 'inline-flex' }}
                >
                  <IconRefresh size={13} />
                </motion.span>
              </IconButton>
              <IconButton label="Settings" onClick={() => setSettingsOpen(true)}>
                <IconSettings size={13} />
              </IconButton>
              <IconButton label="Collapse details" onClick={toggleViewMode}>
                <IconCollapse size={13} />
              </IconButton>
              <IconButton label="Close" onClick={closeWindow}>
                <X size={13} />
              </IconButton>
            </div>
          </header>

          {/* Tab content */}
          <div
            ref={scrollRef}
            data-testid="report-scroll"
            role="tabpanel"
            id={PANEL_ID}
            aria-labelledby={tabDomId(activeTab)}
            // Focusable so the keyboard can reach and scroll a panel that has
            // no focusable content of its own (Trends, an empty state).
            tabIndex={0}
            onScroll={handleScroll}
            onWheel={stopRestoring}
            onPointerDown={stopRestoring}
            onKeyDown={stopRestoring}
            className="
              flex-1 overflow-y-auto px-[var(--space-2xl)] pb-[var(--space-2xl)] pt-[var(--space-lg)]
              outline-none focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]
              focus-visible:-outline-offset-2
            "
          >
            <motion.div
              ref={contentRef}
              key={`${activeTab}-${tabKey}`}
              variants={tabSlide}
              initial="enter"
              animate="center"
              exit="exit"
              custom={slideDir}
            >
              <TabComponent />
            </motion.div>
          </div>
        </div>
      </div>
      {settingsOpen && <SettingsModal onDismiss={() => setSettingsOpen(false)} />}
    </>
  );
}

const PANEL_ID = 'report-panel';

function tabDomId(id: string): string {
  return `report-tab-${id}`;
}

/* ───────────────────────── TabBar ─────────────────────────
 *
 * Horizontal text nav with one moving underline that slides between tabs
 * (Apple's site nav, Linear's view switcher). Layout-tracked via refs so the
 * underline matches the actual rendered button width — no manual measuring.
 * It fills the header's height, so the underline rests on the header's rule.
 *
 * Keyboard follows the ARIA tabs pattern: the tab list is one stop in the Tab
 * order (the selected tab), and Left/Right/Home/End move between tabs,
 * selecting as they go. Tab from there lands in the panel.
 */
function TabBar({
  activeId,
  onSelect,
  tabs,
}: {
  activeId: string;
  onSelect: (id: string) => void;
  tabs: { id: string; label: string }[];
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const buttonRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [indicator, setIndicator] = useState({ x: 0, w: 0 });

  useLayoutEffect(() => {
    const container = containerRef.current;
    const btn = buttonRefs.current[activeId];
    if (!container || !btn) return;
    const measure = () => {
      const cRect = container.getBoundingClientRect();
      // Use a Range over the button's text node — that hugs the actual
      // glyphs and ignores any width the box adds beyond them (UA padding,
      // border, font side-bearings, letter-spacing trail). Falls back to
      // the button rect if the range is empty (no text yet).
      const range = document.createRange();
      range.selectNodeContents(btn);
      const tRect = range.getBoundingClientRect();
      const rect = tRect.width > 0 ? tRect : btn.getBoundingClientRect();
      setIndicator({ x: rect.left - cRect.left, w: rect.width });
    };
    measure();
    // Window resize and font load can change the active button's metrics
    // after first paint. Without re-measuring, the indicator drifts off the
    // text — most visibly when the system font lazy-resolves and glyph
    // widths shift between fallback and SF Pro.
    const ro = new ResizeObserver(measure);
    ro.observe(btn);
    ro.observe(container);
    return () => ro.disconnect();
  }, [activeId, tabs.length]);

  function handleKeyDown(e: KeyboardEvent<HTMLButtonElement>) {
    const index = tabs.findIndex((t) => t.id === activeId);
    const last = tabs.length - 1;
    const to: Record<string, number> = {
      ArrowRight: index >= last ? 0 : index + 1,
      ArrowLeft: index <= 0 ? last : index - 1,
      Home: 0,
      End: last,
    };
    const next = tabs[to[e.key]];
    if (!next) return;
    e.preventDefault();
    onSelect(next.id);
    buttonRefs.current[next.id]?.focus();
  }

  return (
    <div
      ref={containerRef}
      role="tablist"
      className="relative flex shrink-0 items-stretch gap-[var(--space-xl)]"
    >
      {tabs.map((tab) => {
        const active = activeId === tab.id;
        return (
          <button
            key={tab.id}
            ref={(el) => {
              buttonRefs.current[tab.id] = el;
            }}
            role="tab"
            id={tabDomId(tab.id)}
            aria-selected={active}
            aria-controls={PANEL_ID}
            tabIndex={active ? 0 : -1}
            type="button"
            onClick={() => onSelect(tab.id)}
            onKeyDown={handleKeyDown}
            className={[
              'relative inline-flex items-center',
              // p-0 border-0 are explicit because WKWebView (Tauri on macOS)
              // gives <button> a default 2px–6px UA padding that Tailwind
              // preflight does not fully reset, inflating the measured
              // bounding rect past the visible text and dragging the sliding
              // underline indicator with it.
              'p-0 border-0 bg-transparent',
              'text-[length:var(--text-body)] font-[var(--weight-medium)]',
              'transition-colors duration-[var(--duration-fast)] ease-[var(--ease-out)]',
              'cursor-default',
              active
                ? 'text-[color:var(--color-text)]'
                : 'text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text-secondary)]',
              'focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-2 rounded',
            ].join(' ')}
          >
            {tab.label}
          </button>
        );
      })}
      {/* Sliding underline */}
      <motion.span
        aria-hidden
        className="absolute bottom-0 left-0 h-[2px] rounded-full"
        style={{ background: 'var(--color-accent)' }}
        initial={false}
        animate={{ x: indicator.x, width: indicator.w }}
        transition={{ type: 'spring', stiffness: 380, damping: 32, mass: 0.7 }}
      />
    </div>
  );
}
