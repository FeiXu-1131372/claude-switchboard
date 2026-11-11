import { useId, useMemo, useState, type ReactNode } from 'react';
import { Button } from '../components/ui/Button';
import { Card } from '../components/ui/Card';
import { EmptyState } from '../components/ui/EmptyState';
import { ShareBar } from '../components/ui/ShareBar';
import { SkeletonRows } from '../components/ui/Skeleton';
import { formatCost, formatCount, formatTokens } from '../lib/format';
import { ChevronDown, ChevronRight, IconChart, Search } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useAppStore } from '../lib/store';
import type { CacheStats, ModelStats, ProjectStats } from '../lib/types';
import { useTabData, type TabDataState } from '../lib/useTabData';
import { mergeModels, modelRowLabel, modelValue, splitOther, withShares, type ShareRow } from './breakdown';
import { MODEL_COLOR, modelKey } from './modelDisplay';
import { ReportToolbar } from './ReportToolbar';
import { RANGE_DAYS, RANGE_LABEL, useReportStore, type ReportMetric } from './reportState';
import { isHeadlessProject } from './SessionsTab';

/** Projects listed before "Show all". */
const TOP_PROJECTS = 10;

const MODEL_GRID =
  'grid grid-cols-[minmax(0,1fr)_160px_48px_72px_88px] items-center gap-x-[var(--space-md)]';
const PROJECT_GRID =
  'grid grid-cols-[minmax(0,1fr)_72px_160px_88px] items-center gap-x-[var(--space-md)]';
const ROW = 'min-h-[var(--row-height)] border-t border-[var(--color-border-subtle)] px-[var(--space-md)]';
const HEAD =
  'px-[var(--space-md)] pb-[var(--space-xs)] text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]';
const NUM = 'mono tabular-nums text-right text-[length:var(--text-label)]';

/** A model's colour: its family's, or one neutral for everything else. */
function modelColor(id: string): string {
  return MODEL_COLOR[modelKey(id)] ?? 'var(--color-teal)';
}

function plural(n: number, one: string): string {
  return `${formatCount(n)} ${one}${n === 1 ? '' : 's'}`;
}

/** One titled card. A named region, so a section can be found and skipped to. */
function Section({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  const id = useId();
  return (
    <section aria-labelledby={id}>
      <Card className="flex flex-col overflow-hidden pb-[var(--space-xs)]">
        <div className="flex min-h-[var(--row-height)] items-center justify-between gap-[var(--space-md)] px-[var(--space-md)] pt-[var(--space-sm)] pb-[var(--space-sm)]">
          <h3
            id={id}
            className="text-[length:var(--text-label)] font-[var(--weight-medium)] text-[color:var(--color-text)]"
          >
            {title}
          </h3>
          {aside}
        </div>
        {children}
      </Card>
    </section>
  );
}

/** A section's body while it has nothing to show: loading, failed, or empty. */
function Pending<T>({
  state,
  failed,
  children,
}: {
  state: TabDataState<T>;
  failed: string;
  children: (data: T) => ReactNode;
}) {
  if (state.data !== null) return <>{children(state.data)}</>;
  if (state.error) {
    return (
      <div className="flex items-center justify-between gap-[var(--space-md)] px-[var(--space-md)] pb-[var(--space-sm)]">
        <div className="flex min-w-0 flex-col">
          <span className="text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">{failed}</span>
          <span className="truncate text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            {state.error}
          </span>
        </div>
        <Button variant="ghost" size="sm" onClick={state.reload}>
          Retry
        </Button>
      </div>
    );
  }
  return (
    <div className="px-[var(--space-md)] pb-[var(--space-sm)]">
      <SkeletonRows rows={3} />
    </div>
  );
}

function Note({ children }: { children: ReactNode }) {
  return (
    <p className="px-[var(--space-md)] pb-[var(--space-sm)] text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">
      {children}
    </p>
  );
}

function ModelRow({ share }: { share: ShareRow<ModelStats> }) {
  const { row, pct } = share;
  const color = modelColor(row.model);
  return (
    <div data-testid="model-row" className={`${MODEL_GRID} ${ROW}`}>
      <span className="flex min-w-0 items-center gap-[var(--space-sm)]">
        <span aria-hidden className="size-[var(--space-sm)] shrink-0 rounded-full" style={{ background: color }} />
        <span
          data-testid="model-name"
          title={row.model}
          className="truncate text-[length:var(--text-label)] text-[color:var(--color-text)]"
        >
          {modelRowLabel(row.model)}
        </span>
      </span>
      <ShareBar pct={pct} color={color} />
      <span className={`${NUM} text-[color:var(--color-text-secondary)]`}>{pct.toFixed(1)}%</span>
      <span className={`${NUM} text-[color:var(--color-text-muted)]`}>
        {formatTokens(row.input_tokens + row.output_tokens)}
      </span>
      <span className={`${NUM} text-[color:var(--color-text)]`}>{formatCost(row.cost_usd)}</span>
    </div>
  );
}

function Models({ rows, metric }: { rows: ModelStats[]; metric: ReportMetric }) {
  const [otherOpen, setOtherOpen] = useState(false);
  const { main, other } = useMemo(() => splitOther(withShares(rows, metric)), [rows, metric]);
  if (rows.length === 0) return <Note>No model was used in this range.</Note>;

  const otherPct = other.reduce((sum, s) => sum + s.pct, 0);
  const otherTokens = other.reduce((sum, s) => sum + s.row.input_tokens + s.row.output_tokens, 0);
  const otherCost = other.reduce((sum, s) => sum + s.row.cost_usd, 0);
  const Chevron = otherOpen ? ChevronDown : ChevronRight;

  return (
    <>
      {/* The whole in one line: each model's part of it, largest first. */}
      <div
        aria-hidden
        className="mx-[var(--space-md)] mb-[var(--space-md)] flex h-[var(--bar-xl)] gap-px overflow-hidden rounded-[var(--radius-bar)]"
      >
        {main.map((s) => (
          <div key={s.row.model} style={{ width: `${s.pct}%`, background: modelColor(s.row.model) }} />
        ))}
        {other.length > 0 && <div style={{ width: `${otherPct}%`, background: 'var(--color-text-muted)' }} />}
      </div>

      <div className={`${MODEL_GRID} ${HEAD}`}>
        <span>Model</span>
        <span className="col-span-2">Share of {metric === 'cost' ? 'spend' : 'tokens'}</span>
        <span className="text-right">Tokens</span>
        <span className="text-right">Spent</span>
      </div>
      {main.map((s) => (
        <ModelRow key={s.row.model} share={s} />
      ))}
      {other.length > 0 && (
        <>
          <button
            type="button"
            aria-expanded={otherOpen}
            onClick={() => setOtherOpen((open) => !open)}
            className={`${MODEL_GRID} ${ROW} w-full text-left transition-[background] duration-[var(--duration-fast)] hover:bg-[var(--color-bg-card-hover)]`}
          >
            <span className="flex min-w-0 items-center gap-[var(--space-xs)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)]">
              <Chevron size={12} aria-hidden className="shrink-0 text-[color:var(--color-text-muted)]" />
              Other ({other.length})
            </span>
            <ShareBar pct={otherPct} color="var(--color-text-muted)" />
            <span className={`${NUM} text-[color:var(--color-text-secondary)]`}>{otherPct.toFixed(1)}%</span>
            <span className={`${NUM} text-[color:var(--color-text-muted)]`}>{formatTokens(otherTokens)}</span>
            <span className={`${NUM} text-[color:var(--color-text)]`}>{formatCost(otherCost)}</span>
          </button>
          {otherOpen && other.map((s) => <ModelRow key={s.row.model} share={s} />)}
        </>
      )}
    </>
  );
}

function projectName(project: string): string {
  return isHeadlessProject(project) ? 'headless' : project;
}

function Projects({ rows, query }: { rows: ProjectStats[]; query: string }) {
  const [showAll, setShowAll] = useState(false);
  const needle = query.trim().toLowerCase();
  const largest = rows.reduce((max, p) => Math.max(max, p.total_cost_usd), 0);
  const visible = needle
    ? rows.filter((p) => projectName(p.project).toLowerCase().includes(needle))
    : showAll
      ? rows
      : rows.slice(0, TOP_PROJECTS);

  if (rows.length === 0) return <Note>No project was worked on in this range.</Note>;

  return (
    <>
      <div className={`${PROJECT_GRID} ${HEAD}`}>
        <span>Project</span>
        <span className="text-right">Sessions</span>
        <span className="col-span-2 text-right">Spent</span>
      </div>
      {visible.map((p) => {
        const headless = isHeadlessProject(p.project);
        return (
          <div key={p.project} data-testid="project-row" className={`${PROJECT_GRID} ${ROW}`}>
            <span
              data-testid="project-name"
              title={headless ? 'Sessions started without a working directory' : p.project}
              className={[
                'truncate text-[length:var(--text-label)]',
                headless ? 'italic text-[color:var(--color-text-muted)]' : 'text-[color:var(--color-text)]',
              ].join(' ')}
            >
              {projectName(p.project)}
            </span>
            <span className={`${NUM} text-[color:var(--color-text-muted)]`}>{formatCount(p.session_count)}</span>
            <ShareBar pct={largest > 0 ? (p.total_cost_usd / largest) * 100 : 0} />
            <span className={`${NUM} text-[color:var(--color-text)]`}>{formatCost(p.total_cost_usd)}</span>
          </div>
        );
      })}
      {needle && visible.length === 0 && (
        <div className={`${ROW} flex items-center text-[length:var(--text-label)] text-[color:var(--color-text-muted)]`}>
          No project matches “{query.trim()}”
        </div>
      )}
      {!needle && rows.length > TOP_PROJECTS && (
        <div className={`${ROW} flex items-center`}>
          {/* Pulled left by its own padding, so its label starts under the names. */}
          <Button
            variant="ghost"
            size="sm"
            className="-ml-[var(--space-sm)]"
            onClick={() => setShowAll((all) => !all)}
          >
            {showAll ? `Show top ${TOP_PROJECTS}` : `Show all ${formatCount(rows.length)}`}
          </Button>
        </div>
      )}
    </>
  );
}

function Figure({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div title={hint} className="flex flex-col gap-[var(--space-2xs)]">
      <span className="text-[length:var(--text-micro)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-muted)]">
        {label}
      </span>
      <span className="mono tabular-nums text-[length:var(--text-body)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
        {value}
      </span>
    </div>
  );
}

function Cache({ stats }: { stats: CacheStats }) {
  const cached = stats.total_cache_read_tokens + stats.total_cache_creation_tokens;
  if (cached === 0) return <Note>Nothing was cached in this range.</Note>;
  return (
    <div className="grid grid-cols-4 gap-[var(--space-md)] px-[var(--space-md)] pb-[var(--space-sm)]">
      <Figure
        label="Hit rate"
        value={`${(stats.hit_ratio * 100).toFixed(1)}%`}
        hint="Cached tokens read back, out of all cached tokens read or written"
      />
      <Figure label="Reads" value={formatTokens(stats.total_cache_read_tokens)} hint="Tokens read from the cache" />
      <Figure label="Writes" value={formatTokens(stats.total_cache_creation_tokens)} hint="Tokens written to the cache" />
      <Figure
        label="Est. saved"
        value={formatCost(stats.estimated_savings_usd)}
        hint="What the cache reads would have cost at the full input price, less what they did cost"
      />
    </div>
  );
}

/**
 * Where the usage went: by model, by project, and how much the cache carried.
 *
 * These were three tabs with three layouts and three hidden ranges. Here they
 * are three sections of one page for the one shared range, each a plain table
 * whose number columns line up down the page. Each section loads on its own,
 * so one failing leaves the other two standing.
 */
export function BreakdownTab() {
  const version = useAppStore((s) => s.sessionDataVersion);
  const range = useReportStore((s) => s.range);
  const metric = useReportStore((s) => s.metric);
  const days = RANGE_DAYS[range];

  const models = useTabData(() => ipc.getModelBreakdown(days), [version], `models:${days}`);
  const projects = useTabData(() => ipc.getProjectBreakdown(days), [version], `projects:${days}`);
  const cache = useTabData(() => ipc.getCacheStats(days), [version], `cache:${days}`);
  const [projectQuery, setProjectQuery] = useState('');

  const merged = useMemo(() => (models.data ? mergeModels(models.data) : null), [models.data]);

  // The headline is the models' total; projects add up to the same spend, so
  // they stand in for it if the models could not be loaded.
  const spent = merged
    ? merged.reduce((sum, m) => sum + m.cost_usd, 0)
    : projects.data
      ? projects.data.reduce((sum, p) => sum + p.total_cost_usd, 0)
      : null;
  const tokens = merged ? merged.reduce((sum, m) => sum + modelValue(m, 'tokens'), 0) : null;
  const figure =
    metric === 'cost'
      ? spent === null ? '—' : formatCost(spent)
      : tokens === null ? '—' : `${formatTokens(tokens)} tokens`;
  const caption = [
    RANGE_LABEL[range],
    merged ? plural(merged.length, 'model') : null,
    projects.data ? plural(projects.data.length, 'project') : null,
  ]
    .filter(Boolean)
    .join(' · ');

  const toolbar = <ReportToolbar figure={figure} caption={caption} showMetric />;
  const states = [models, projects, cache];

  if (states.every((s) => s.loading)) {
    return (
      <div className="flex flex-col gap-[var(--space-md)]">
        {toolbar}
        <SkeletonRows />
      </div>
    );
  }

  const nothing =
    states.every((s) => !s.loading && !s.error) &&
    merged?.length === 0 &&
    projects.data?.length === 0 &&
    (cache.data?.total_cache_read_tokens ?? 0) + (cache.data?.total_cache_creation_tokens ?? 0) === 0;
  if (nothing) {
    return (
      <div className="flex flex-col gap-[var(--space-md)]">
        {toolbar}
        <EmptyState
          icon={<IconChart size={32} />}
          title="Nothing in this range"
          description="Models, projects and cache use appear here as you use Claude Code."
        />
      </div>
    );
  }

  const manyProjects = (projects.data?.length ?? 0) > TOP_PROJECTS;

  return (
    <div className="flex flex-col gap-[var(--space-md)]">
      {toolbar}

      <Section title="By model">
        <Pending state={{ ...models, data: merged }} failed="Couldn't load models">
          {(rows) => <Models rows={rows} metric={metric} />}
        </Pending>
      </Section>

      <Section
        title="By project"
        aside={
          manyProjects && (
            <label className="relative block w-[200px]">
              <Search
                size={12}
                aria-hidden
                className="pointer-events-none absolute left-[var(--space-sm)] top-1/2 -translate-y-1/2 text-[color:var(--color-text-muted)]"
              />
              <input
                type="search"
                aria-label="Filter projects"
                placeholder="Filter projects"
                value={projectQuery}
                onChange={(e) => setProjectQuery(e.target.value)}
                className="
                  w-full rounded-[var(--radius-sm)]
                  border border-[var(--color-border)] bg-[var(--color-bg-base)]
                  py-[var(--space-2xs)] pl-[26px] pr-[var(--space-sm)]
                  text-[length:var(--text-label)] text-[color:var(--color-text)]
                  placeholder:text-[color:var(--color-text-muted)]
                  transition-[border-color] duration-[var(--duration-fast)]
                  focus:border-[var(--color-border-focus)] focus:outline-none
                "
              />
            </label>
          )
        }
      >
        <Pending state={projects} failed="Couldn't load projects">
          {(rows) => <Projects rows={rows} query={manyProjects ? projectQuery : ''} />}
        </Pending>
      </Section>

      <Section title="Cache">
        <Pending state={cache} failed="Couldn't load cache stats">
          {(stats) => <Cache stats={stats} />}
        </Pending>
      </Section>
    </div>
  );
}
