import { useMemo } from 'react';
import { Button } from '../components/ui/Button';
import { ModelBadge } from '../components/ui/ModelBadge';
import { EmptyState } from '../components/ui/EmptyState';
import { SkeletonRows } from '../components/ui/Skeleton';
import { formatTokens, formatCost, formatCount } from '../lib/format';
import { ChevronDown, ChevronRight, IconCompact, IconSessions, IconSubagent } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useTabData } from '../lib/useTabData';
import { useAppStore } from '../lib/store';
import type { Compaction, SessionCostRow, SubagentCost } from '../lib/types';
import { RESUMABLE_SESSIONS_KEY } from '../sessions/useResumableSessions';
import { useReportStore } from './reportState';

// `modelLabel` now lives in ./modelDisplay alongside modelKey/MODEL_VARIANT so
// the shared <ModelBadge> can reach it without importing a tab component.
// Re-exported here because this module was its original public home.
export { modelLabel } from './modelDisplay';

/** A session whose transcript had no working directory (`cwd` absent) lands
 * in ~/.claude/projects/-/ and resolves to project "-". These are headless /
 * scheduled `claude` invocations (regular cadence, 1 turn, tiny tokens), not
 * interactive work — so relabel them "headless" and visually demote the row
 * instead of showing a bare "-" that reads as a broken session. */
export function isHeadlessProject(project: string): boolean {
  return !project || project === '-';
}

function formatClock(iso: string): string {
  return new Date(iso).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' });
}

/** Local calendar day of `iso` as `YYYY-MM-DD`.
 *
 * Built from the Date's local getters rather than `toISOString()` (which is
 * UTC) or a locale format (which varies by machine), because this string is
 * the grouping key for a row's cost. It has to agree with the backend's
 * `get_daily_trends`, which buckets on `chrono::Local` — otherwise the Cost
 * tab and the Trends bar for the same day would disagree near midnight. */
function localDayKey(iso: string): string {
  const d = new Date(iso);
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${month}-${day}`;
}

function formatDayLabel(dayKey: string): string {
  const today = localDayKey(new Date().toISOString());
  if (dayKey === today) return 'Today';
  const yesterday = new Date();
  yesterday.setDate(yesterday.getDate() - 1);
  if (dayKey === localDayKey(yesterday.toISOString())) return 'Yesterday';
  // Parse as local midnight — `new Date('2026-07-26')` would be parsed as UTC
  // and could render the previous day west of Greenwich.
  const [y, m, d] = dayKey.split('-').map(Number);
  return new Date(y, m - 1, d).toLocaleDateString('en-US', {
    month: 'short',
    day: 'numeric',
  });
}

/** Splitting rows by day multiplies their count, so the cap is higher than
 *  the old per-conversation 100 — it still bounds the DOM on a heavy month. */
const MAX_ROWS = 250;

/** Days of history this tab aggregates. Mirrored by `TAB_WINDOW_DAYS` in
 *  ExpandedReport so the header label cannot drift from what is queried. */
export const WINDOW_DAYS = 7;

/** Subagent transcripts live at `<project>/<sessionId>/subagents/agent-*.jsonl`
 * and belong to the sibling `<sessionId>.jsonl`. Same rule as the backend's
 * `session_file_of`, which is what a cost row's id is built from. */
function parentKeyOf(sourceFile: string): string {
  const idx = sourceFile.indexOf('/subagents/');
  // The sibling parent transcript is `<sessionId>.jsonl` in the project dir,
  // i.e. everything before `/subagents/` plus a `.jsonl` suffix.
  return idx < 0 ? sourceFile : sourceFile.slice(0, idx) + '.jsonl';
}

/** Cost rows are keyed `<transcript path>#<day>`. `listResumableSessions`
 * identifies each session by its transcript's file stem, so that same stem on
 * a row id is the join key for the session's title. */
function sessionIdOfRowId(rowId: string): string {
  const hash = rowId.lastIndexOf('#');
  const file = (hash >= 0 ? rowId.slice(0, hash) : rowId).replace(/\\/g, '/');
  const name = file.slice(file.lastIndexOf('/') + 1);
  return name.endsWith('.jsonl') ? name.slice(0, -'.jsonl'.length) : name;
}

/** The cost row a compaction belongs to: the same `<parent transcript>#<day>`
 * the backend gives the row, so a compaction recorded in a subagent's
 * transcript still lands on the conversation that started it. */
export function compactionRowId(c: Pick<Compaction, 'source_file' | 'ts'>): string {
  return `${parentKeyOf(c.source_file)}#${localDayKey(c.ts)}`;
}

const NO_ROWS: SessionCostRow[] = [];

const BREAKDOWN_ROWS: Array<{
  key: keyof SessionCostRow['breakdown'];
  costKey: keyof SessionCostRow['cost_breakdown'];
  label: string;
}> = [
  { key: 'input', costKey: 'input', label: 'Input' },
  { key: 'output', costKey: 'output', label: 'Output' },
  { key: 'cache_read', costKey: 'cache_read', label: 'Cache read' },
  { key: 'cache_write_5m', costKey: 'cache_write_5m', label: 'Cache write (5m)' },
  { key: 'cache_write_1h', costKey: 'cache_write_1h', label: 'Cache write (1h)' },
];

function BreakdownTable({
  breakdown,
  costBreakdown,
}: {
  breakdown: SessionCostRow['breakdown'];
  costBreakdown: SessionCostRow['cost_breakdown'];
}) {
  // Hide rows with zero tokens — keeps the table tight, since most
  // sessions don't use the 1h cache tier at all.
  const rows = BREAKDOWN_ROWS.filter((r) => breakdown[r.key] > 0);
  return (
    <div
      className={[
        'mx-[var(--space-sm)] mb-[var(--space-sm)]',
        'rounded-[var(--radius-md)] border border-[var(--color-border-subtle)]',
        'bg-[var(--color-bg-card)]',
      ].join(' ')}
    >
      {rows.map((r, i) => (
        <div
          key={r.key}
          className={[
            'flex items-center justify-between gap-[var(--space-md)] px-[var(--space-md)] py-[var(--space-xs)]',
            i < rows.length - 1 ? 'border-b border-[var(--color-border-subtle)]' : '',
          ].join(' ')}
        >
          <span className="flex-1 text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            {r.label}
          </span>
          <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] tabular-nums min-w-[64px] text-right">
            {formatTokens(breakdown[r.key])}
          </span>
          <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-muted)] tabular-nums min-w-[56px] text-right">
            {formatCost(costBreakdown[r.costKey])}
          </span>
        </div>
      ))}
    </div>
  );
}

/** Compaction boundaries that fall inside one session-day row.
 *
 * Worth surfacing because a compaction is invisible in the turn count but
 * explains two things a bare row cannot: why a single day's session appears
 * to hold more history than its turns suggest, and why cost steps up right
 * afterwards — the surviving context is re-written to a cold cache at full
 * price on the next call. */
function CompactionRows({ items }: { items: Compaction[] }) {
  return (
    <div className="flex flex-col">
      {items.map((c) => {
        const dropped = c.pre_tokens > c.post_tokens ? c.pre_tokens - c.post_tokens : 0;
        return (
          <div
            key={c.uuid}
            className="
              flex items-center gap-[var(--space-sm)]
              pl-[calc(var(--space-sm)+14px+var(--space-sm))] pr-[var(--space-sm)]
              py-[var(--space-2xs)]
            "
          >
            <IconCompact
              size={12}
              aria-hidden
              className="shrink-0 text-[color:var(--color-warn-text)] opacity-80"
            />
            <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-secondary)]">
              Context compacted{c.trigger === 'manual' ? '' : ' automatically'} at{' '}
              {formatClock(c.ts)}
            </span>
            <span className="mono text-[length:var(--text-micro)] tabular-nums text-[color:var(--color-text-muted)]">
              {formatTokens(c.pre_tokens)} → {formatTokens(c.post_tokens)}
            </span>
            {dropped > 0 && (
              <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                ({formatTokens(dropped)} dropped)
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}

/** An indented, muted row for one subagent under its parent session. */
function SubagentRow({ sub }: { sub: SubagentCost }) {
  return (
    <div
      className={[
        'flex items-center gap-[var(--space-sm)]',
        'pl-[calc(var(--space-sm)+14px+var(--space-sm))] pr-[var(--space-sm)] py-[var(--space-2xs)]',
      ].join(' ')}
    >
      <IconSubagent size={12} className="shrink-0 text-[color:var(--color-text-muted)] opacity-60" />
      <span className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] truncate flex-1 min-w-0">
        {sub.label}
      </span>
      <ModelBadge model={sub.dominant_model} />
      <span className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] tabular-nums min-w-[64px] text-right">
        {formatTokens(sub.headline_tokens)}
      </span>
      <span className="mono text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] tabular-nums min-w-[48px] text-right">
        {formatCost(sub.total_cost_usd)}
      </span>
    </div>
  );
}

export function SessionsTab() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const { data, error, loading, reload } = useTabData(
    () =>
      Promise.all([ipc.getSessionCosts(WINDOW_DAYS), ipc.getCompactions(WINDOW_DAYS)]).then(
        ([rows, compactions]) => ({ rows, compactions }),
      ),
    [version],
    `cost:${WINDOW_DAYS}`,
  );
  // One row per conversation per day, summed by the backend, newest first.
  const sessions = data?.rows ?? NO_ROWS;
  // In the report store so the open row survives leaving the tab.
  const expandedId = useReportStore((s) => s.costOpenRow);
  const setExpandedId = useReportStore((s) => s.setCostOpenRow);

  // Session titles come from the same summaries the Sessions tab renders,
  // and share its cached copy. Refreshed once per mount on purpose, not on
  // `version`: that would re-run the backend's transcript scan on every
  // ingestion tick (roughly every turn while a session is active), and the
  // scan cache is invalidated by exactly that activity. Best-effort: on
  // failure the rows keep their project labels rather than losing the tab.
  const summaries = useTabData(() => ipc.listResumableSessions(), [], RESUMABLE_SESSIONS_KEY).data;
  const titles = useMemo(() => {
    const map = new Map<string, string>();
    for (const r of summaries ?? []) {
      const title = r.title?.trim();
      if (title) map.set(r.session_id, title);
    }
    return map;
  }, [summaries]);

  const totalCost = useMemo(
    () => sessions.reduce((sum, s) => sum + s.total_cost_usd, 0),
    [sessions],
  );

  /** Compactions keyed exactly like a session row (`<parentFile>#<day>`), so
   * an expanded row can look up its own boundaries in O(1). */
  const compactionsByRow = useMemo(() => {
    const map = new Map<string, Compaction[]>();
    for (const c of data?.compactions ?? []) {
      const key = compactionRowId(c);
      const list = map.get(key);
      if (list) list.push(c);
      else map.set(key, [c]);
    }
    for (const list of map.values()) {
      list.sort((a, b) => (a.ts < b.ts ? -1 : 1));
    }
    return map;
  }, [data?.compactions]);

  /** Distinct conversations, not rows: a conversation worked on across three
   * days is one session that produced three rows. Counting rows here would
   * inflate the headline the same way the old cost attribution did. */
  const conversationCount = useMemo(
    () => new Set(sessions.map((s) => s.id.slice(0, s.id.lastIndexOf('#')))).size,
    [sessions],
  );

  /** Rows bucketed under their day, newest first. `sessions` is already
   * sorted by `latest_ts` descending and every row covers a single day, so a
   * linear pass produces day groups in order without a second sort. */
  const days = useMemo(() => {
    const out: Array<{ day: string; rows: SessionCostRow[]; cost: number }> = [];
    for (const s of sessions.slice(0, MAX_ROWS)) {
      let group = out[out.length - 1];
      if (!group || group.day !== s.day) {
        group = { day: s.day, rows: [], cost: 0 };
        out.push(group);
      }
      group.rows.push(s);
      group.cost += s.total_cost_usd;
    }
    return out;
  }, [sessions]);

  if (error) {
    return (
      <EmptyState
        icon={<IconSessions size={32} />}
        title="Couldn't load sessions"
        description={error}
        action={<Button variant="ghost" size="sm" onClick={reload}>Retry</Button>}
      />
    );
  }
  if (loading || !data) {
    return <SkeletonRows />;
  }

  if (sessions.length === 0) {
    return (
      <EmptyState
        icon={<IconSessions size={32} />}
        title="No sessions yet"
        description="Sessions will appear here as you use Claude Code."
      />
    );
  }

  return (
    <div className="flex flex-col gap-[var(--space-sm)]">
      <div className="flex items-center justify-between px-[var(--space-2xs)]">
        <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">
          {formatCount(conversationCount)} {conversationCount === 1 ? 'session' : 'sessions'}
        </span>
        <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
          {formatCost(totalCost)}
        </span>
      </div>

      <div className="flex flex-col">
        {days.map((group) => (
          <div key={group.day} className="flex flex-col">
            {/* The day total is the point of the header, not decoration: it is
                what the Trends bar for this date shows, so the two tabs can be
                reconciled by eye. */}
            <div
              className="
                sticky top-0 z-[1] flex items-baseline justify-between
                bg-[var(--color-bg-base)] px-[var(--space-sm)]
                pt-[var(--space-md)] pb-[var(--space-2xs)]
              "
            >
              <span
                className="
                  text-[length:var(--text-micro)] font-[var(--weight-medium)]
                  uppercase tracking-[var(--tracking-label)]
                  text-[color:var(--color-text-muted)]
                "
              >
                {formatDayLabel(group.day)}
              </span>
              <span className="mono text-[length:var(--text-micro)] tabular-nums text-[color:var(--color-text-muted)]">
                {formatCost(group.cost)}
              </span>
            </div>
            {group.rows.map((session) => {
          const isOpen = expandedId === session.id;
          const Chevron = isOpen ? ChevronDown : ChevronRight;
          const agentCount = session.subagents.length;
          const headless = isHeadlessProject(session.project);
          const compactions = compactionsByRow.get(session.id) ?? [];
          const title = titles.get(sessionIdOfRowId(session.id));
          const projectLabel = headless ? 'headless' : session.project;
          // The title is what makes a row recognisable — the Sessions tab
          // leads with it too. Project stays in the meta line for rows that
          // have a title, since day grouping says nothing about the project.
          const label = title ?? projectLabel;
          const meta = [
            title ? projectLabel : null,
            formatClock(session.latest_ts),
            `${formatCount(session.turn_count)} ${session.turn_count === 1 ? 'turn' : 'turns'}`,
            agentCount > 0 ? `${agentCount} ${agentCount === 1 ? 'agent' : 'agents'}` : null,
          ]
            .filter(Boolean)
            .join(' · ');
          return (
            <div key={session.id} className="border-b border-[var(--color-border-subtle)]">
              <button
                type="button"
                onClick={() => setExpandedId(isOpen ? null : session.id)}
                className={[
                  'w-full flex items-center gap-[var(--space-sm)]',
                  'px-[var(--space-sm)] py-[var(--space-sm)]',
                  'text-left',
                  'transition-[background] duration-[var(--duration-fast)]',
                  'hover:bg-[var(--color-bg-card)]',
                  isOpen ? 'bg-[var(--color-bg-card)]' : '',
                  headless ? 'opacity-55' : '',
                ].join(' ')}
                aria-expanded={isOpen}
              >
                <Chevron
                  size={14}
                  className="shrink-0 text-[color:var(--color-text-muted)]"
                />
                <div className="flex flex-col min-w-0 flex-1">
                  <div className="flex items-center gap-[var(--space-sm)]">
                    <span
                      className={[
                        'text-[length:var(--text-body)] truncate',
                        headless && !title
                          ? 'italic text-[color:var(--color-text-muted)]'
                          : 'text-[color:var(--color-text)]',
                      ].join(' ')}
                    >
                      {label}
                    </span>
                    <ModelBadge model={session.dominant_model} />
                    {/* Collapsed-row marker: without it the boundary is only
                        discoverable by expanding every row one at a time. */}
                    {compactions.length > 0 && (
                      <span
                        title={`Context compacted ${compactions.length}×`}
                        className="inline-flex shrink-0 items-center text-[color:var(--color-warn-text)] opacity-80"
                      >
                        <IconCompact size={11} aria-label="Context compacted" />
                      </span>
                    )}
                  </div>
                  <span className="truncate text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                    {meta}
                  </span>
                </div>

                <div className="flex items-center gap-[var(--space-md)] shrink-0">
                  <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] tabular-nums">
                    {formatTokens(session.headline_tokens)}
                  </span>
                  <span className="mono text-[length:var(--text-label)] text-[color:var(--color-text-muted)] tabular-nums min-w-[48px] text-right">
                    {formatCost(session.total_cost_usd)}
                  </span>
                </div>
              </button>
              {isOpen && (
                <>
                  {compactions.length > 0 && <CompactionRows items={compactions} />}
                  {agentCount > 0 && (
                    <div className="flex flex-col py-[var(--space-2xs)]">
                      {session.subagents.map((sub) => (
                        <SubagentRow key={sub.id} sub={sub} />
                      ))}
                    </div>
                  )}
                  <BreakdownTable
                    breakdown={session.breakdown}
                    costBreakdown={session.cost_breakdown}
                  />
                </>
              )}
            </div>
          );
            })}
          </div>
        ))}
        {sessions.length > MAX_ROWS && (
          <div className="py-[var(--space-md)] text-center text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            Showing the latest {MAX_ROWS} rows.
          </div>
        )}
      </div>
    </div>
  );
}
