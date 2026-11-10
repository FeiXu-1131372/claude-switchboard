import { useMemo } from 'react';
import type { SessionSummary } from '../lib/generated/bindings';
import { Banner } from '../components/ui/Banner';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { EmptyState } from '../components/ui/EmptyState';
import { SkeletonRows } from '../components/ui/Skeleton';
import { formatCost } from '../lib/format';
import { ChevronDown, IconSessions, Search, X } from '../lib/icons';
import { modelLabel } from '../report/modelDisplay';
import { useResumableSessions } from './useResumableSessions';
import { SessionRow } from './SessionRow';
import { useResume } from './useResume';
import { DEFAULT_SESSIONS_VIEW, useReportStore, type SortOption, type TimeFilter } from '../report/reportState';

export type { SortOption, TimeFilter } from '../report/reportState';

function matches(s: SessionSummary, q: string): boolean {
  const hay = [
    s.title,
    s.project_name,
    s.git_branch ?? '',
    s.model ?? '',
    s.recap ?? '',
    s.asked,
    s.left_off ?? '',
    ...s.touched_files,
  ]
    .join(' ')
    .toLowerCase();
  const tokens = q.split(/\s+/).filter(Boolean);
  return tokens.every((token) => hay.includes(token));
}

function getTime(s: SessionSummary): number {
  const t = new Date(s.ended_at || s.started_at).getTime();
  return Number.isFinite(t) ? t : 0;
}

function isWithinTimeRange(s: SessionSummary, filter: TimeFilter): boolean {
  if (filter === 'all') return true;
  const ts = getTime(s);
  if (!ts) return false;
  const now = Date.now();
  if (filter === 'today') {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    return ts >= today.getTime();
  }
  if (filter === '7d') {
    return ts >= now - 7 * 86_400_000;
  }
  if (filter === '30d') {
    return ts >= now - 30 * 86_400_000;
  }
  return true;
}

function sortSessions(list: SessionSummary[], sort: SortOption): SessionSummary[] {
  const copy = [...list];
  switch (sort) {
    case 'recent':
      return copy.sort((a, b) => getTime(b) - getTime(a));
    case 'oldest':
      return copy.sort((a, b) => getTime(a) - getTime(b));
    case 'turns':
      return copy.sort((a, b) => b.turns - a.turns || getTime(b) - getTime(a));
    case 'tokens':
      return copy.sort((a, b) => b.total_tokens - a.total_tokens || getTime(b) - getTime(a));
    case 'cost':
      return copy.sort((a, b) => b.total_cost_usd - a.total_cost_usd || getTime(b) - getTime(a));
    case 'peak_context':
      return copy.sort(
        (a, b) => (b.peak_context_tokens ?? 0) - (a.peak_context_tokens ?? 0) || getTime(b) - getTime(a),
      );
    case 'title':
      return copy.sort((a, b) => a.title.localeCompare(b.title) || getTime(b) - getTime(a));
    default:
      return copy;
  }
}

const controlSelectClass = [
  'appearance-none cursor-pointer',
  'rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-bg-card)]',
  'py-[2px] pl-[var(--space-xs)] pr-[20px]',
  'text-[length:var(--text-micro)] text-[color:var(--color-text)]',
  'transition-[border-color,background] duration-[var(--duration-fast)]',
  'hover:border-[var(--color-border-hover)]',
  'focus:border-[var(--color-border-focus)] focus:outline-none',
].join(' ');

/** Rows share one bordered surface with hairline dividers, the way the Cost
 *  tab lists sessions. One card per row — the previous shape — put a border
 *  and a gap around every single line, which is what made the list read as
 *  undifferentiated blocks instead of a scannable column. */
function RowGroup({ children }: { children: React.ReactNode }) {
  return (
    <Card className="overflow-hidden [&>*+*]:border-t [&>*+*]:border-[var(--color-border-subtle)]">
      {children}
    </Card>
  );
}

export function SessionsBrowserTab() {
  const { sessions, loading, error } = useResumableSessions();
  // Search, sort, filters and the open row live in the report store, not
  // in this component: the tab unmounts whenever you leave it, and losing
  // a half-typed search to a glance at another tab is the bug this fixes.
  const view = useReportStore((s) => s.sessions);
  const patch = useReportStore((s) => s.patchSessions);
  const {
    query,
    sortOption,
    projectFilter,
    modelFilter,
    timeFilter,
    hasRecapOnly,
    resumableOnly,
    groupByProject,
    expanded,
  } = view;
  const { resume, dialog, notice, vsCodeAvailable } = useResume();

  const q = query.trim().toLowerCase();

  const projectOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of sessions) {
      if (s.project_name) {
        counts.set(s.project_name, (counts.get(s.project_name) ?? 0) + 1);
      }
    }
    return [...counts.entries()]
      .map(([project, count]) => ({ project, count }))
      .sort((a, b) => a.project.localeCompare(b.project));
  }, [sessions]);

  const modelOptions = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of sessions) {
      if (s.model) {
        counts.set(s.model, (counts.get(s.model) ?? 0) + 1);
      }
    }
    return [...counts.entries()]
      .map(([model, count]) => ({
        model,
        label: modelLabel(model),
        count,
      }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [sessions]);

  const filtered = useMemo(() => {
    let list = sessions;

    if (q) {
      list = list.filter((s) => matches(s, q));
    }
    if (projectFilter !== 'all') {
      list = list.filter((s) => s.project_name === projectFilter);
    }
    if (modelFilter !== 'all') {
      list = list.filter((s) => s.model === modelFilter);
    }
    if (timeFilter !== 'all') {
      list = list.filter((s) => isWithinTimeRange(s, timeFilter));
    }
    if (hasRecapOnly) {
      list = list.filter((s) => Boolean(s.recap?.trim()));
    }
    if (resumableOnly) {
      list = list.filter((s) => s.cwd_exists);
    }

    return sortSessions(list, sortOption);
  }, [sessions, q, projectFilter, modelFilter, timeFilter, hasRecapOnly, resumableOnly, sortOption]);

  const shouldGroup = groupByProject && !q;

  // Search flattens the grouping: a two-result query should not be split
  // across two project headers.
  const groups = useMemo(() => {
    if (!shouldGroup) return null;
    const by = new Map<string, SessionSummary[]>();
    for (const s of filtered) {
      const list = by.get(s.project_name) ?? [];
      list.push(s);
      by.set(s.project_name, list);
    }
    return [...by.entries()];
  }, [filtered, shouldGroup]);

  const isNonSearchFiltered =
    projectFilter !== 'all' ||
    modelFilter !== 'all' ||
    timeFilter !== 'all' ||
    hasRecapOnly ||
    resumableOnly;

  const isFiltered = Boolean(query || isNonSearchFiltered);

  function clearFilters() {
    const { query, projectFilter, modelFilter, timeFilter, hasRecapOnly, resumableOnly } =
      DEFAULT_SESSIONS_VIEW;
    patch({ query, projectFilter, modelFilter, timeFilter, hasRecapOnly, resumableOnly });
  }

  function toggle(id: string) {
    patch({ expanded: expanded === id ? null : id });
  }

  const rows = (list: SessionSummary[], inGroup = false) =>
    list.map((s) => (
      <SessionRow
        key={s.session_id}
        session={s}
        expanded={expanded === s.session_id}
        showProject={!inGroup}
        vsCodeAvailable={vsCodeAvailable}
        onToggle={toggle}
        onResume={(_id, surface) => resume(s, surface)}
      />
    ));

  return (
    <div className="flex flex-col gap-[var(--space-md)]">
      <div className="flex flex-col gap-[var(--space-xs)]">
        {/* Search input + Session count */}
        <div className="flex items-center gap-[var(--space-md)]">
          <div className="relative min-w-0 flex-1">
            <Search
              size={13}
              aria-hidden
              className="
                pointer-events-none absolute left-[var(--space-sm)] top-1/2 -translate-y-1/2
                text-[color:var(--color-text-muted)]
              "
            />
            <input
              aria-label="Search sessions"
              placeholder="Search titles, recaps, files, branches…"
              value={query}
              onChange={(e) => patch({ query: e.target.value })}
              className="
                w-full rounded-[var(--radius-sm)]
                border border-[var(--color-border)] bg-[var(--color-bg-card)]
                py-[var(--space-xs)] pl-[26px] pr-[28px]
                text-[length:var(--text-body)] text-[color:var(--color-text)]
                placeholder:text-[color:var(--color-text-muted)]
                transition-[border-color] duration-[var(--duration-fast)]
                focus:border-[var(--color-border-focus)] focus:outline-none
              "
            />
            {query && (
              <button
                type="button"
                onClick={() => patch({ query: '' })}
                aria-label="Clear"
                className="
                  absolute right-[var(--space-xs)] top-1/2 -translate-y-1/2
                  p-[3px] text-[color:var(--color-text-muted)]
                  hover:text-[color:var(--color-text)]
                  rounded-[var(--radius-sm)]
                  transition-colors duration-[var(--duration-fast)]
                "
              >
                <X size={12} aria-hidden />
              </button>
            )}
          </div>
          {sessions.length > 0 && (
            <span className="mono shrink-0 text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] tabular-nums">
              {filtered.length} of {sessions.length}
            </span>
          )}
        </div>

        {/* Filter and Sort controls */}
        {sessions.length > 0 && (
          <div className="flex flex-wrap items-center gap-x-[var(--space-md)] gap-y-[var(--space-xs)] pt-[2px]">
            {/* Sort */}
            <label className="flex items-center gap-[var(--space-xs)]">
              <span className="text-[length:var(--text-micro)] font-[var(--weight-medium)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
                Sort
              </span>
              <div className="relative inline-flex items-center">
                <select
                  aria-label="Sort sessions by"
                  value={sortOption}
                  onChange={(e) => patch({ sortOption: e.target.value as SortOption })}
                  className={controlSelectClass}
                >
                  <option value="recent">Most recent</option>
                  <option value="oldest">Oldest</option>
                  <option value="turns">Most turns</option>
                  <option value="tokens">Most tokens</option>
                  <option value="cost">Highest cost</option>
                  <option value="peak_context">Peak context</option>
                  <option value="title">Title (A–Z)</option>
                </select>
                <ChevronDown
                  size={11}
                  aria-hidden
                  className="pointer-events-none absolute right-[6px] top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]"
                />
              </div>
            </label>

            {/* Project Filter */}
            {projectOptions.length > 0 && (
              <label className="flex items-center gap-[var(--space-xs)]">
                <span className="text-[length:var(--text-micro)] font-[var(--weight-medium)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
                  Project
                </span>
                <div className="relative inline-flex items-center max-w-[170px]">
                  <select
                    aria-label="Filter by project"
                    value={projectFilter}
                    onChange={(e) => patch({ projectFilter: e.target.value })}
                    className={`${controlSelectClass} truncate`}
                  >
                    <option value="all">All ({projectOptions.length})</option>
                    {projectOptions.map(({ project, count }) => (
                      <option key={project} value={project}>
                        {project} ({count})
                      </option>
                    ))}
                  </select>
                  <ChevronDown
                    size={11}
                    aria-hidden
                    className="pointer-events-none absolute right-[6px] top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]"
                  />
                </div>
              </label>
            )}

            {/* Model Filter */}
            {modelOptions.length > 0 && (
              <label className="flex items-center gap-[var(--space-xs)]">
                <span className="text-[length:var(--text-micro)] font-[var(--weight-medium)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
                  Model
                </span>
                <div className="relative inline-flex items-center max-w-[160px]">
                  <select
                    aria-label="Filter by model"
                    value={modelFilter}
                    onChange={(e) => patch({ modelFilter: e.target.value })}
                    className={`${controlSelectClass} truncate`}
                  >
                    <option value="all">All models</option>
                    {modelOptions.map(({ model, label, count }) => (
                      <option key={model} value={model}>
                        {label} ({count})
                      </option>
                    ))}
                  </select>
                  <ChevronDown
                    size={11}
                    aria-hidden
                    className="pointer-events-none absolute right-[6px] top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]"
                  />
                </div>
              </label>
            )}

            {/* Time Filter */}
            <label className="flex items-center gap-[var(--space-xs)]">
              <span className="text-[length:var(--text-micro)] font-[var(--weight-medium)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
                Time
              </span>
              <div className="relative inline-flex items-center">
                <select
                  aria-label="Filter by time period"
                  value={timeFilter}
                  onChange={(e) => patch({ timeFilter: e.target.value as TimeFilter })}
                  className={controlSelectClass}
                >
                  <option value="all">All time</option>
                  <option value="today">Today</option>
                  <option value="7d">Last 7 days</option>
                  <option value="30d">Last 30 days</option>
                </select>
                <ChevronDown
                  size={11}
                  aria-hidden
                  className="pointer-events-none absolute right-[6px] top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]"
                />
              </div>
            </label>

            {/* Quick toggle chips */}
            <div className="flex items-center gap-[var(--space-xs)]">
              <button
                type="button"
                onClick={() => patch({ hasRecapOnly: !hasRecapOnly })}
                aria-pressed={hasRecapOnly}
                className={[
                  'inline-flex items-center px-[7px] py-[2px] rounded-[var(--radius-pill)]',
                  'text-[length:var(--text-micro)] font-[var(--weight-medium)] select-none',
                  'transition-[background,color,border-color] duration-[var(--duration-fast)] cursor-pointer',
                  hasRecapOnly
                    ? 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent-text)] border border-[var(--color-accent)]/30 font-[var(--weight-semibold)]'
                    : 'bg-[var(--color-bg-card)] text-[color:var(--color-text-muted)] border border-[var(--color-border)] hover:text-[color:var(--color-text)] hover:border-[var(--color-border-hover)]',
                ].join(' ')}
              >
                Recap only
              </button>

              <button
                type="button"
                onClick={() => patch({ resumableOnly: !resumableOnly })}
                aria-pressed={resumableOnly}
                className={[
                  'inline-flex items-center px-[7px] py-[2px] rounded-[var(--radius-pill)]',
                  'text-[length:var(--text-micro)] font-[var(--weight-medium)] select-none',
                  'transition-[background,color,border-color] duration-[var(--duration-fast)] cursor-pointer',
                  resumableOnly
                    ? 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent-text)] border border-[var(--color-accent)]/30 font-[var(--weight-semibold)]'
                    : 'bg-[var(--color-bg-card)] text-[color:var(--color-text-muted)] border border-[var(--color-border)] hover:text-[color:var(--color-text)] hover:border-[var(--color-border-hover)]',
                ].join(' ')}
              >
                Resumable only
              </button>

              <button
                type="button"
                onClick={() => patch({ groupByProject: !groupByProject })}
                aria-pressed={groupByProject}
                className={[
                  'inline-flex items-center px-[7px] py-[2px] rounded-[var(--radius-pill)]',
                  'text-[length:var(--text-micro)] font-[var(--weight-medium)] select-none',
                  'transition-[background,color,border-color] duration-[var(--duration-fast)] cursor-pointer',
                  groupByProject
                    ? 'bg-[var(--color-accent-dim)] text-[color:var(--color-accent-text)] border border-[var(--color-accent)]/30 font-[var(--weight-semibold)]'
                    : 'bg-[var(--color-bg-card)] text-[color:var(--color-text-muted)] border border-[var(--color-border)] hover:text-[color:var(--color-text)] hover:border-[var(--color-border-hover)]',
                ].join(' ')}
              >
                Group by project
              </button>
            </div>

            {/* Clear filters button */}
            {isFiltered && (
              <button
                type="button"
                onClick={clearFilters}
                className="
                  inline-flex items-center gap-[3px]
                  px-[6px] py-[2px] rounded-[var(--radius-sm)]
                  text-[length:var(--text-micro)] font-[var(--weight-medium)]
                  text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]
                  transition-colors duration-[var(--duration-fast)] cursor-pointer
                "
              >
                <X size={11} aria-hidden />
                <span>Clear filters</span>
              </button>
            )}
          </div>
        )}
      </div>

      {error && <Banner variant="error">{error}</Banner>}
      {notice && (
        <Banner variant="warning" role="status">
          {notice}
        </Banner>
      )}

      {loading && sessions.length === 0 && <SkeletonRows />}

      {!loading && !error && sessions.length === 0 && (
        <EmptyState
          icon={<IconSessions size={32} />}
          title="No sessions yet"
          description="Sessions you run with Claude Code will appear here."
        />
      )}
      {!loading && sessions.length > 0 && filtered.length === 0 && (
        <div className="flex flex-col items-center justify-center py-[var(--space-xl)] gap-[var(--space-sm)] text-center">
          <p className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">
            {query && !isNonSearchFiltered
              ? `No sessions match “${query}”.`
              : `No sessions match the selected filters${query ? ` for “${query}”` : ''}.`}
          </p>
          {isFiltered && (
            <Button variant="ghost" size="sm" onClick={clearFilters}>
              Reset filters
            </Button>
          )}
        </div>
      )}

      {groups
        ? groups.map(([project, list]) => {
            // Sums the same visible rows the group contains, so the header
            // reconciles with what the user can see (filters included). Rows
            // hide their cost when no usage was ingested, so those sessions
            // contribute nothing here either.
            const groupCost = list.reduce(
              (sum, s) => sum + (s.total_tokens > 0 ? s.total_cost_usd : 0),
              0,
            );
            return (
              <section key={project} className="flex flex-col gap-[var(--space-xs)]">
                <header className="flex items-baseline justify-between gap-[var(--space-sm)] px-[2px]">
                  <div className="flex min-w-0 items-baseline gap-[var(--space-sm)]">
                    <h3
                      className="
                        truncate text-[length:var(--text-label)] font-[var(--weight-semibold)]
                        uppercase tracking-[var(--tracking-label)]
                        text-[color:var(--color-text-secondary)]
                      "
                    >
                      {project}
                    </h3>
                    <span className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] tabular-nums">
                      {list.length}
                    </span>
                  </div>
                  {/* Same rule as the rows: a group with no ingested usage
                      stays blank rather than claiming a $0.00 measurement. */}
                  {groupCost > 0 && (
                    <span className="mono shrink-0 text-[length:var(--text-micro)] text-[color:var(--color-text-secondary)] tabular-nums">
                      {formatCost(groupCost)}
                    </span>
                  )}
                </header>
                <RowGroup>{rows(list, true)}</RowGroup>
              </section>
            );
          })
        : filtered.length > 0 && <RowGroup>{rows(filtered, false)}</RowGroup>}

      {dialog}
    </div>
  );
}
