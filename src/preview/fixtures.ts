/**
 * Fixture data for the browser preview (`preview.html`). Never imported by
 * the app: only `src/preview/main.tsx` and tests reach this file.
 *
 * Everything the mocked commands return is summed from one ledger of usage
 * cells, so the totals on different tabs reconcile for any time window, the
 * way they do in the real app where every command reads the same events.
 */
import pricingFile from '../../src-tauri/pricing.json';
import { costPerCategory, lookupPricing } from '../lib/pricing';
import type {
  AccountListEntry,
  CacheStats,
  CachedUsage,
  DailyBucket,
  DailyModelBucket,
  ModelStats,
  PricingEntry,
  ProjectStats,
  Provider,
  SessionCostRow,
  SessionSummary,
  Settings,
  StoredCompaction,
  StoredSessionEvent,
  SubagentCost,
  Utilization,
} from '../lib/generated/bindings';

export type PreviewScale = 'heavy' | 'light' | 'empty';

/** One conversation's use of one model on one day. */
export interface UsageCell {
  ts: number;
  dayStart: number;
  project: string;
  sessionId: string;
  model: string;
  cost: number;
  input: number;
  output: number;
  cacheRead: number;
  cache5m: number;
  cache1h: number;
}

export interface PreviewData {
  accounts: AccountListEntry[];
  settings: Settings;
  /** Raw events for the last 7 days. */
  events7: StoredSessionEvent[];
  compactions7: StoredCompaction[];
  /** Every day of history. */
  daily: DailyBucket[];
  dailyModels: DailyModelBucket[];
  /** The 30-day window, as the old fixed-range tabs asked for it. */
  models: ModelStats[];
  projects: ProjectStats[];
  cache: CacheStats;
  sessions: SessionSummary[];
  providers: Provider[];
  /** The ledger every aggregate above is summed from; see `windowOf`. */
  ledger: UsageCell[];
}

export interface PreviewWindow {
  daily: DailyBucket[];
  dailyModels: DailyModelBucket[];
  models: ModelStats[];
  projects: ProjectStats[];
  cache: CacheStats;
  events: StoredSessionEvent[];
  sessionCosts: SessionCostRow[];
  compactions: StoredCompaction[];
}

const DAY_MS = 86_400_000;
const HOME = '/Users/dev';

/** Sorted the way `get_pricing` returns it: longest prefix first. */
export const PRICING: PricingEntry[] = [...(pricingFile.pricing as PricingEntry[])].sort(
  (a, b) => b.prefix.length - a.prefix.length,
);

interface ScaleSpec {
  projects: number;
  days: number;
  models: number;
  /** Target spend over the most recent 30 calendar days. */
  spend30: number;
}

const SPEC: Record<PreviewScale, ScaleSpec> = {
  heavy: { projects: 300, days: 136, models: 13, spend30: 13_000 },
  light: { projects: 6, days: 18, models: 4, spend30: 96 },
  empty: { projects: 0, days: 0, models: 0, spend30: 0 },
};

/** Share of spend per model: one dominant model and a long tail under 1%. */
const MODELS: ReadonlyArray<{ id: string; weight: number }> = [
  { id: 'claude-opus-5-5', weight: 62 },
  { id: 'claude-fable-5-2', weight: 18 },
  { id: 'claude-fable-5-1', weight: 7 },
  { id: 'claude-sonnet-5-5', weight: 5 },
  { id: 'claude-haiku-5-5', weight: 2.4 },
  { id: 'claude-opus-4-7', weight: 2 },
  { id: 'claude-sonnet-4-6', weight: 1.4 },
  { id: 'glm-5.3', weight: 0.9 },
  { id: 'claude-haiku-4-5-20251001', weight: 0.5 },
  { id: 'deepseek-v4.1-flash-1017', weight: 0.35 },
  { id: 'claude-fable-5-1-reversed', weight: 0.25 },
  { id: 'anthropic/claude-fable-5.1', weight: 0.12 },
  { id: 'kimi-k3.1', weight: 0.08 },
];

const HEAD_PROJECTS = [
  'claude-switchboard',
  'storefront',
  'billing-service',
  'common-ai-backend',
  'marketing-plan',
  'ingest-pipeline',
  '-',
  'customer-data-platform-migration-2026-q4',
  'design-tokens',
  'mobile-app',
  'infra-terraform',
  'docs-site',
];
const STEMS = [
  'atlas', 'ledger', 'harbor', 'quartz', 'ember', 'nimbus', 'orchard', 'pylon', 'relay', 'sable',
  'tundra', 'vector', 'willow', 'zephyr', 'beacon', 'cobalt', 'delta', 'falcon', 'garnet', 'helix',
];
const KINDS = [
  'api', 'web', 'worker', 'cli', 'sdk', 'docs', 'infra', 'etl', 'admin', 'mobile', 'proxy', 'bot',
  'lab', 'tools', 'console',
];

const TITLES = [
  'Fix the flaky webhook retry test',
  'Add CSV export to the billing report',
  'Why does the nightly sync skip archived rows?',
  'Refactor the auth middleware into one module',
  'Upgrade to React 19 and fix the type errors',
  'Investigate the memory growth in the worker',
  'Write the migration for the invoices table',
  'Review the pull request for rate limiting',
  'Make the onboarding form accessible',
  'Speed up the cold start of the CLI',
  'Document the deploy runbook',
  'Add retries with backoff to the uploader',
  'Trace the 502s from the edge proxy',
  'Replace moment with date-fns across the app',
  'Draft the Q4 roadmap summary',
  'Port the settings screen to the new tokens',
];
const FILES = [
  'src/app.tsx', 'src/lib/http.ts', 'src/routes/billing.ts', 'README.md', 'package.json',
  'server/worker.rs', 'server/queue.rs', 'migrations/0042_invoices.sql', 'tests/webhook.test.ts',
  'docs/runbook.md', 'src/components/Form.tsx', 'infra/main.tf',
];

function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** FNV-1a, so an id depends only on its key and never on generation order. */
function hash(key: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

function hex(rng: () => number, length: number): string {
  let out = '';
  for (let i = 0; i < length; i++) out += Math.floor(rng() * 16).toString(16);
  return out;
}

function uuidFrom(key: string): string {
  const r = mulberry32(hash(key));
  return `${hex(r, 8)}-${hex(r, 4)}-4${hex(r, 3)}-a${hex(r, 3)}-${hex(r, 12)}`;
}

function projectName(i: number): string {
  if (i < HEAD_PROJECTS.length) return HEAD_PROJECTS[i];
  const j = i - HEAD_PROJECTS.length;
  return `${STEMS[j % STEMS.length]}-${KINDS[Math.floor(j / STEMS.length) % KINDS.length]}`;
}

function dayKey(ms: number): string {
  const d = new Date(ms);
  const month = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${month}-${day}`;
}

function transcriptPath(project: string, sessionId: string): string {
  const dir = project === '-' ? '-' : `-Users-dev-code-${project}`;
  return `${HOME}/.claude/projects/${dir}/${sessionId}.jsonl`;
}

/** How often project `i` is worked on: the head daily, the tail once a month. */
function period(i: number, spec: ScaleSpec): number {
  if (spec.projects <= 10) return i < 2 ? 1 : i < 4 ? 2 : 3;
  if (i < 10) return 1;
  if (i < 25) return 2;
  if (i < 60) return 4;
  if (i < 120) return 8;
  if (i < 200) return 15;
  return 30;
}

function tokensFor(model: string, cost: number) {
  const entry = lookupPricing(PRICING, model);
  const rate = {
    input: entry?.input_per_mtok ?? 1,
    output: entry?.output_per_mtok ?? 4,
    cacheRead: entry?.cache_read_per_mtok ?? 0.1,
    cache5m: entry?.cache_5m_per_mtok ?? 1.25,
    cache1h: entry?.cache_1h_per_mtok ?? 2,
  };
  const of = (share: number, perMtok: number) =>
    perMtok > 0 ? Math.round(((cost * share) / perMtok) * 1_000_000) : 0;
  return {
    input: of(0.07, rate.input),
    output: of(0.21, rate.output),
    cacheRead: of(0.46, rate.cacheRead),
    cache5m: of(0.22, rate.cache5m),
    cache1h: of(0.04, rate.cache1h),
  };
}

function buildLedger(spec: ScaleSpec, now: Date): UsageCell[] {
  if (spec.days === 0) return [];
  const rng = mulberry32(7);
  const nowMs = now.getTime();
  const models = MODELS.slice(0, spec.models);
  const modelWeight = models.reduce((s, m) => s + m.weight, 0);
  const weights = Array.from({ length: spec.projects }, (_, i) => 1 / Math.pow(i + 1, 1.25));

  // Relative volume per day, k days back from today: quiet early history, a
  // ramp over the last six weeks, quieter weekends and one very large day.
  const volume: number[] = [];
  for (let k = 0; k < spec.days; k++) {
    const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - k);
    const weekend = start.getDay() === 0 || start.getDay() === 6;
    const ramp = k < 20 ? 1 : k < 42 ? 1 - ((k - 20) / 22) * 0.85 : 0.1 + rng() * 0.08;
    const spike = k === 4 ? 3.1 : 1;
    volume.push(ramp * (weekend ? 0.55 : 1) * (0.6 + rng() * 0.8) * spike);
  }
  const recent = volume.slice(0, 30).reduce((s, v) => s + v, 0);
  const perUnit = spec.spend30 / recent;

  const cells: UsageCell[] = [];
  for (let k = 0; k < spec.days; k++) {
    const dayStart = new Date(now.getFullYear(), now.getMonth(), now.getDate() - k).getTime();
    const span = k === 0 ? Math.max(nowMs - dayStart, 0) : DAY_MS;

    const active: Array<{ i: number; share: number }> = [];
    for (let i = 0; i < spec.projects; i++) {
      if (k >= 30 && i >= 60) continue;
      if ((7 * k + 13 * i) % period(i, spec) !== 0) continue;
      active.push({ i, share: weights[i] * (0.5 + rng()) });
    }
    const shareSum = active.reduce((s, a) => s + a.share, 0);

    for (const { i, share } of active) {
      const projectCost = (volume[k] * perUnit * share) / shareSum;
      const project = projectName(i);
      const sessionCount = i < 3 ? 3 : i < 10 ? 2 : 1;
      for (let s = 0; s < sessionCount; s++) {
        // The head projects' first conversation runs across two days.
        const block = i < 5 && s === 0 ? Math.floor((k + i) / 2) : k;
        const sessionId = uuidFrom(`${i}:${block}:${s}`);
        const sessionCost = projectCost * (s === 0 ? (sessionCount === 1 ? 1 : 0.6) : 0.4 / (sessionCount - 1));

        let pick = (mulberry32(hash(`m:${sessionId}`))() * modelWeight);
        let primary = models[0].id;
        for (const m of models) {
          pick -= m.weight;
          if (pick <= 0) {
            primary = m.id;
            break;
          }
        }
        const helper = models.find((m) => m.id.includes('haiku'))?.id;
        const split: Array<[string, number]> =
          helper && helper !== primary && sessionCost > 5
            ? [[primary, 0.96], [helper, 0.04]]
            : [[primary, 1]];

        const at = dayStart + (0.3 + rng() * 0.65) * span;
        for (const [model, part] of split) {
          const cost = sessionCost * part;
          cells.push({ ts: Math.floor(at), dayStart, project, sessionId, model, cost, ...tokensFor(model, cost) });
        }
      }
    }
  }
  return cells;
}

/** Events for the given cells. Recent cells become several API calls, some
 * from subagents; older ones a single call.
 * Costs and tokens always sum back to the cell. */
function eventsOf(cells: UsageCell[], now: Date): StoredSessionEvent[] {
  const recentFrom = now.getTime() - 7 * DAY_MS;
  const events: StoredSessionEvent[] = [];
  for (const c of cells) {
    const key = `${c.sessionId}:${c.model}:${c.dayStart}`;
    const rng = mulberry32(hash(key));
    const parent = transcriptPath(c.project, c.sessionId);
    const calls = c.ts < recentFrom ? 1 : Math.min(9, 1 + Math.floor(c.cost / 12) + Math.floor(rng() * 2));
    const agents = calls >= 4 ? [hex(rng, 16), hex(rng, 16)] : [];

    let left = { cost: c.cost, input: c.input, output: c.output, cacheRead: c.cacheRead, cache5m: c.cache5m, cache1h: c.cache1h };
    for (let j = 0; j < calls; j++) {
      const last = j === calls - 1;
      const f = last ? 1 : 1 / (calls - j);
      const take = (n: number) => (last ? n : Math.floor(n * f));
      const part = {
        cost: last ? left.cost : left.cost * f,
        input: take(left.input),
        output: take(left.output),
        cacheRead: take(left.cacheRead),
        cache5m: take(left.cache5m),
        cache1h: take(left.cache1h),
      };
      left = {
        cost: left.cost - part.cost,
        input: left.input - part.input,
        output: left.output - part.output,
        cacheRead: left.cacheRead - part.cacheRead,
        cache5m: left.cache5m - part.cache5m,
        cache1h: left.cache1h - part.cache1h,
      };
      const agent = agents.length > 0 && j > 0 && j % 3 !== 0 ? agents[j % agents.length] : null;
      const source = agent
        ? `${parent.slice(0, -'.jsonl'.length)}/subagents/agent-${agent}.jsonl`
        : parent;
      events.push({
        ts: new Date(Math.max(c.dayStart, c.ts - j * 180_000)).toISOString(),
        project: c.project,
        model: c.model,
        input_tokens: part.input,
        output_tokens: part.output,
        cache_read_tokens: part.cacheRead,
        cache_creation_5m_tokens: part.cache5m,
        cache_creation_1h_tokens: part.cache1h,
        cost_usd: part.cost,
        source_file: source,
        source_line: j + 1,
        event_id: `req_${hex(rng, 12)}:msg_${hex(rng, 12)}`,
      });
    }
  }
  return events;
}

/** The model with the most output; the first one seen on a tie. */
function dominantOf(output: Map<string, number>): string {
  let best = '';
  let most = -1;
  for (const [model, n] of output) {
    if (n > most) {
      most = n;
      best = model;
    }
  }
  return best;
}

/** Stand-in for the backend's `session_costs::aggregate`: one row per
 * conversation per local day, subagents listed under the session that started
 * them, newest first. */
function sessionCostsOf(events: StoredSessionEvent[]): SessionCostRow[] {
  type Sub = { row: SubagentCost; output: Map<string, number> };
  const parents = new Map<string, { row: SessionCostRow; output: Map<string, number>; subs: Map<string, Sub> }>();
  for (const e of events) {
    const at = e.source_file.indexOf('/subagents/');
    const day = dayKey(Date.parse(e.ts));
    const id = `${at < 0 ? e.source_file : `${e.source_file.slice(0, at)}.jsonl`}#${day}`;
    let p = parents.get(id);
    if (!p) {
      p = {
        row: {
          id,
          project: e.project,
          day,
          latest_ts: e.ts,
          turn_count: 0,
          headline_tokens: 0,
          total_cost_usd: 0,
          dominant_model: e.model,
          breakdown: { input: 0, output: 0, cache_read: 0, cache_write_5m: 0, cache_write_1h: 0 },
          cost_breakdown: { input: 0, output: 0, cache_read: 0, cache_write_5m: 0, cache_write_1h: 0 },
          subagents: [],
        },
        output: new Map(),
        subs: new Map(),
      };
      parents.set(id, p);
    }
    const r = p.row;
    r.turn_count += 1;
    r.headline_tokens += e.input_tokens + e.output_tokens;
    r.total_cost_usd += e.cost_usd;
    if (e.ts > r.latest_ts) r.latest_ts = e.ts;
    r.breakdown.input += e.input_tokens;
    r.breakdown.output += e.output_tokens;
    r.breakdown.cache_read += e.cache_read_tokens;
    r.breakdown.cache_write_5m += e.cache_creation_5m_tokens;
    r.breakdown.cache_write_1h += e.cache_creation_1h_tokens;
    p.output.set(e.model, (p.output.get(e.model) ?? 0) + e.output_tokens);
    const entry = lookupPricing(PRICING, e.model);
    if (entry) {
      const c = costPerCategory(entry, {
        input: e.input_tokens,
        output: e.output_tokens,
        cache_read: e.cache_read_tokens,
        cache_5m: e.cache_creation_5m_tokens,
        cache_1h: e.cache_creation_1h_tokens,
      });
      r.cost_breakdown.input += c.input;
      r.cost_breakdown.output += c.output;
      r.cost_breakdown.cache_read += c.cache_read;
      r.cost_breakdown.cache_write_5m += c.cache_5m;
      r.cost_breakdown.cache_write_1h += c.cache_1h;
    }
    if (at >= 0) {
      let s = p.subs.get(e.source_file);
      if (!s) {
        const hexId = /agent-([0-9a-f]+)/i.exec(e.source_file)?.[1];
        s = {
          row: {
            id: e.source_file,
            label: hexId ? `agent ${hexId.slice(0, 8)}` : 'subagent',
            latest_ts: e.ts,
            turn_count: 0,
            headline_tokens: 0,
            total_cost_usd: 0,
            dominant_model: e.model,
          },
          output: new Map(),
        };
        p.subs.set(e.source_file, s);
      }
      s.row.turn_count += 1;
      s.row.headline_tokens += e.input_tokens + e.output_tokens;
      s.row.total_cost_usd += e.cost_usd;
      if (e.ts > s.row.latest_ts) s.row.latest_ts = e.ts;
      s.output.set(e.model, (s.output.get(e.model) ?? 0) + e.output_tokens);
    }
  }
  const rows = [...parents.values()].map((p) => ({
    ...p.row,
    dominant_model: dominantOf(p.output),
    subagents: [...p.subs.values()]
      .map((s) => ({ ...s.row, dominant_model: dominantOf(s.output) }))
      .sort((a, b) => a.latest_ts.localeCompare(b.latest_ts) || a.id.localeCompare(b.id)),
  }));
  return rows.sort((a, b) => b.latest_ts.localeCompare(a.latest_ts) || a.id.localeCompare(b.id));
}

function compactionsOf(cells: UsageCell[]): StoredCompaction[] {
  const out: StoredCompaction[] = [];
  const seen = new Set<string>();
  for (const c of cells) {
    const key = `${c.sessionId}:${c.dayStart}`;
    if (c.cost < 60 || seen.has(key)) continue;
    seen.add(key);
    const rng = mulberry32(hash(`compact:${key}`));
    out.push({
      ts: new Date(Math.max(c.dayStart, c.ts - 240_000)).toISOString(),
      source_file: transcriptPath(c.project, c.sessionId),
      trigger: rng() < 0.7 ? 'auto' : 'manual',
      pre_tokens: 620_000 + Math.floor(rng() * 340_000),
      post_tokens: 28_000 + Math.floor(rng() * 30_000),
      uuid: uuidFrom(`compact:${key}`),
    });
  }
  return out;
}

function modelStats(cells: UsageCell[]): ModelStats[] {
  const by = new Map<string, ModelStats>();
  for (const c of cells) {
    let m = by.get(c.model);
    if (!m) {
      m = { model: c.model, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 0 };
      by.set(c.model, m);
    }
    m.input_tokens += c.input;
    m.output_tokens += c.output;
    m.cache_read_tokens += c.cacheRead;
    m.cache_creation_tokens += c.cache5m + c.cache1h;
    m.cost_usd += c.cost;
  }
  return [...by.values()].sort((a, b) => b.cost_usd - a.cost_usd);
}

function aggregate(cells: UsageCell[]) {
  const daily = new Map<string, DailyBucket>();
  const dailyCells = new Map<string, UsageCell[]>();
  const projects = new Map<string, { cost: number; sessions: Set<string> }>();
  let read = 0;
  let written = 0;
  let saved = 0;
  for (const c of cells) {
    const date = dayKey(c.ts);
    let d = daily.get(date);
    if (!d) {
      d = { date, input_tokens: 0, output_tokens: 0, cost_usd: 0 };
      daily.set(date, d);
      dailyCells.set(date, []);
    }
    d.input_tokens += c.input;
    d.output_tokens += c.output;
    d.cost_usd += c.cost;
    dailyCells.get(date)!.push(c);

    let p = projects.get(c.project);
    if (!p) {
      p = { cost: 0, sessions: new Set() };
      projects.set(c.project, p);
    }
    p.cost += c.cost;
    p.sessions.add(c.sessionId);

    read += c.cacheRead;
    written += c.cache5m + c.cache1h;
    const entry = lookupPricing(PRICING, c.model);
    if (entry) saved += (c.cacheRead / 1_000_000) * (entry.input_per_mtok - entry.cache_read_per_mtok);
  }
  const dates = [...daily.keys()].sort();
  const cache: CacheStats = {
    total_cache_read_tokens: read,
    total_cache_creation_tokens: written,
    estimated_savings_usd: saved,
    hit_ratio: read + written > 0 ? read / (read + written) : 0,
  };
  const projectStats: ProjectStats[] = [...projects.entries()]
    .map(([project, p]) => ({ project, session_count: p.sessions.size, total_cost_usd: p.cost }))
    .sort((a, b) => b.total_cost_usd - a.total_cost_usd || a.project.localeCompare(b.project));
  return {
    daily: dates.map((date) => daily.get(date)!),
    dailyModels: dates.map((date) => ({ date, models: modelStats(dailyCells.get(date)!) })),
    models: modelStats(cells),
    projects: projectStats,
    cache,
  };
}

/** Local midnight at the start of a report covering `days` days, today being
 * the last of them. Mirrors `report_window_start` in the backend. */
export function reportStart(days: number, now: Date): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() - (Math.max(days, 1) - 1)).getTime();
}

/** What the backend would return for a report of `days` whole days ending today. */
export function windowOf(data: PreviewData, days: number, now: Date): PreviewWindow {
  const from = reportStart(days, now);
  const cells = data.ledger.filter((c) => c.ts >= from);
  const recent = cells.filter((c) => c.ts >= now.getTime() - 7 * DAY_MS);
  const events = eventsOf(cells, now);
  return { ...aggregate(cells), events, sessionCosts: sessionCostsOf(events), compactions: compactionsOf(recent) };
}

function sessionSummaries(cells: UsageCell[], limit: number): SessionSummary[] {
  const by = new Map<string, UsageCell[]>();
  for (const c of cells) {
    const list = by.get(c.sessionId);
    if (list) list.push(c);
    else by.set(c.sessionId, [c]);
  }
  const rows: SessionSummary[] = [];
  for (const [sessionId, list] of by) {
    const rng = mulberry32(hash(`summary:${sessionId}`));
    const project = list[0].project;
    const started = Math.min(...list.map((c) => c.ts));
    const ended = Math.max(...list.map((c) => c.ts));
    const cost = list.reduce((s, c) => s + c.cost, 0);
    const byModel = new Map<string, number>();
    for (const c of list) byModel.set(c.model, (byModel.get(c.model) ?? 0) + c.cost);
    const model = [...byModel.entries()].sort((a, b) => b[1] - a[1])[0][0];
    const title = TITLES[Math.floor(rng() * TITLES.length)];
    const headless = project === '-';
    const fileCount = Math.floor(rng() * 5);
    rows.push({
      session_id: sessionId,
      cwd: headless ? '' : `${HOME}/code/${project}`,
      project_name: project,
      git_branch: headless ? null : rng() < 0.55 ? 'main' : `feat/${title.toLowerCase().split(' ').slice(1, 4).join('-')}`,
      title: headless ? 'Scheduled run' : title,
      recap:
        rng() < 0.5
          ? `Goal: ${title.toLowerCase()}. The change is written and the tests pass locally; the remaining step is to run the full suite in CI and open the pull request.`
          : null,
      asked: `${title}. Start by reading the existing code, then propose the smallest change that works.`,
      left_off: rng() < 0.6 ? 'Tests pass. Next: open the pull request and ask for review.' : null,
      touched_files: Array.from({ length: fileCount }, (_, n) => FILES[(Math.floor(rng() * FILES.length) + n) % FILES.length]),
      touched_overflow: fileCount === 4 ? Math.floor(rng() * 12) : 0,
      model,
      peak_context_tokens: 30_000 + Math.floor(rng() * rng() * 700_000),
      turns: 3 + Math.floor(cost * (2 + rng() * 3)),
      started_at: new Date(started - Math.floor(rng() * 5_400_000)).toISOString(),
      ended_at: new Date(ended).toISOString(),
      total_tokens: list.reduce((s, c) => s + c.input + c.output, 0),
      total_cost_usd: cost,
      permission_mode: rng() < 0.3 ? 'acceptEdits' : rng() < 0.15 ? 'bypassPermissions' : null,
      cwd_exists: !headless && rng() < 0.94,
    });
  }
  rows.sort((a, b) => (a.ended_at < b.ended_at ? 1 : -1));
  return rows.slice(0, limit);
}

function window(pct: number, resetsInHours: number | null, now: Date): Utilization {
  return {
    utilization: pct,
    resets_at: resetsInHours == null ? null : new Date(now.getTime() + resetsInHours * 3_600_000).toISOString(),
  };
}

function account(
  now: Date,
  o: {
    slot: number;
    email: string;
    plan: string | null;
    active?: boolean;
    source?: AccountListEntry['source'];
    usage?: Pick<CachedUsage['snapshot'], 'five_hour' | 'seven_day' | 'seven_day_fable'>;
    error?: string;
  },
): AccountListEntry {
  const uuid = uuidFrom(`account:${o.slot}`);
  const cached: CachedUsage | null = o.usage
    ? {
        snapshot: {
          five_hour: o.usage.five_hour,
          seven_day: o.usage.seven_day,
          seven_day_sonnet: null,
          seven_day_opus: null,
          seven_day_fable: o.usage.seven_day_fable,
          extra_usage: null,
          fetched_at: new Date(now.getTime() - 90_000).toISOString(),
        },
        account_id: uuid,
        account_email: o.email,
        last_error: null,
        burn_rate: null,
        auth_source: 'ClaudeCode',
        origin: 'http',
      }
    : null;
  return {
    slot: o.slot,
    email: o.email,
    account_uuid: uuid,
    org_name: null,
    org_uuid: null,
    subscription_type: o.plan,
    source: o.source ?? 'ImportedFromClaudeCode',
    is_active: o.active ?? false,
    cached_usage: cached,
    last_error: o.error ?? null,
  };
}

function buildAccounts(now: Date): AccountListEntry[] {
  return [
    // Ultra has no 5-hour window. Nearly out of weekly, Fable untouched.
    account(now, {
      slot: 1,
      email: 'morgan@example.com',
      plan: 'ultra',
      usage: { five_hour: null, seven_day: window(96, 14, now), seven_day_fable: window(0, null, now) },
    }),
    // The active login. Weekly and Fable reset at the same moment.
    account(now, {
      slot: 2,
      email: 'riley.chen@example.com',
      plan: 'ultra',
      active: true,
      usage: { five_hour: null, seven_day: window(45, 51, now), seven_day_fable: window(45, 51, now) },
    }),
    account(now, {
      slot: 3,
      email: 'CI token',
      plan: null,
      source: 'LongLivedToken',
      error: 'usage_not_supported',
    }),
    account(now, {
      slot: 4,
      email: 'sam@example.com',
      plan: 'pro',
      usage: { five_hour: window(0, null, now), seven_day: window(85, 30, now), seven_day_fable: null },
    }),
  ];
}

const SETTINGS: Settings = {
  polling_interval_secs: 300,
  stagger_gap_secs: 20,
  thresholds: [75, 90],
  theme: 'light',
  launch_at_login: false,
  crash_reports: false,
  preferred_auth_source: null,
  terminal: null,
  account_rotation: null,
};

const PROVIDERS: Provider[] = [
  { id: 'official', name: 'Anthropic', kind: 'official', base_url: null, auth_token: null, env: {}, extra_args: [], preset_id: null, sort_index: 0 },
  {
    id: 'relay-example',
    name: 'Example relay',
    kind: 'third_party',
    base_url: 'https://relay.example.com',
    auth_token: 'example-token',
    env: { ANTHROPIC_MODEL: 'glm-5.3' },
    extra_args: [],
    preset_id: null,
    sort_index: 1,
  },
];

export function buildFixtures(scale: PreviewScale, now: Date): PreviewData {
  const ledger = buildLedger(SPEC[scale], now);
  const data: PreviewData = {
    accounts: buildAccounts(now),
    settings: SETTINGS,
    events7: [],
    compactions7: [],
    daily: [],
    dailyModels: [],
    models: [],
    projects: [],
    cache: { total_cache_read_tokens: 0, total_cache_creation_tokens: 0, estimated_savings_usd: 0, hit_ratio: 0 },
    sessions: sessionSummaries(ledger, 300),
    providers: PROVIDERS,
    ledger,
  };
  const all = aggregate(ledger);
  const week = windowOf(data, 7, now);
  const month = windowOf(data, 30, now);
  return {
    ...data,
    events7: week.events,
    compactions7: week.compactions,
    daily: all.daily,
    dailyModels: all.dailyModels,
    models: month.models,
    projects: month.projects,
    cache: month.cache,
  };
}
