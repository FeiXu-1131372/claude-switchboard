/**
 * Browser preview of the real UI with fixture data. Loaded only by
 * `preview.html`, so none of this reaches the production bundle.
 *
 *   pnpm preview:ui → http://localhost:1430/preview.html
 *   ?scale=heavy|demo|light|empty   how much history (default heavy; demo is the README's)
 *   ?theme=light|dark          light or dark mode (default light)
 *   ?palette=<id>              colour theme (default terracotta)
 *   ?view=expanded|compact     which window (default expanded)
 *   ?window=WxH                the window's real size in CSS px, for when the OS gives
 *                              it less than the 960x640 the app asks for (e.g. 834x556)
 *   ?delay=800                 answer report queries that many ms late, to see loading states
 */
import React from 'react';
import ReactDOM from 'react-dom/client';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { buildFixtures, windowOf, PRICING, type PreviewScale } from './fixtures';
import '../styles/globals.css';
import '../styles/tokens.css';

const SCALES: readonly PreviewScale[] = ['heavy', 'demo', 'light', 'empty'];

async function start() {
  const params = new URLSearchParams(window.location.search);
  const scaleParam = params.get('scale') as PreviewScale | null;
  const scale = scaleParam && SCALES.includes(scaleParam) ? scaleParam : 'heavy';
  const theme = params.get('theme') === 'dark' ? 'dark' : 'light';
  // Not checked against the list here: the theme store must not load before
  // the choice is in storage, and it falls back to the original for a name
  // it does not know.
  const palette = params.get('palette') ?? 'terracotta';
  const viewMode = params.get('view') === 'compact' ? 'compact' : 'expanded';
  const delay = Math.max(0, Number(params.get('delay')) || 0);

  try {
    localStorage.setItem('theme-preference', theme);
    localStorage.setItem('theme-palette', palette);
  } catch {
    // Storage blocked: the theme below still applies for this load.
  }
  document.body.dataset.theme = theme;
  document.body.dataset.palette = palette;
  const frame = document.getElementById('frame')!;
  frame.dataset.view = viewMode;
  const windowSize = /^(\d{2,4})x(\d{2,4})$/.exec(params.get('window') ?? '');
  if (windowSize) {
    frame.style.width = `${windowSize[1]}px`;
    frame.style.height = `${windowSize[2]}px`;
  }

  const now = new Date();
  const data = buildFixtures(scale, now);
  // ?active=<slot> signs a different fixture account in, and ?fable=<pct>
  // gives that account a Fable cap on its own reset, so the popover can be
  // looked at for each plan shape without changing the sidebar's fixtures.
  const activeSlot = Number(params.get('active'));
  if (data.accounts.some((a) => a.slot === activeSlot)) {
    data.accounts = data.accounts.map((a) => ({ ...a, is_active: a.slot === activeSlot }));
  }
  const fablePct = Number(params.get('fable'));
  if (params.has('fable') && Number.isFinite(fablePct)) {
    const resetsAt = new Date(now.getTime() + 80 * 3_600_000).toISOString();
    data.accounts = data.accounts.map((a) =>
      a.is_active && a.cached_usage
        ? {
            ...a,
            cached_usage: {
              ...a.cached_usage,
              snapshot: { ...a.cached_usage.snapshot, seven_day_fable: { utilization: fablePct, resets_at: resetsAt } },
            },
          }
        : a,
    );
  }
  let settings = { ...data.settings, theme };
  const days = (payload: unknown) => (payload as { days: number }).days;

  mockWindows('main');
  const answer = (cmd: string, payload: unknown): unknown => {
      switch (cmd) {
        case 'get_current_usage':
          return data.accounts.find((a) => a.is_active)?.cached_usage ?? null;
        case 'list_accounts':
          return data.accounts;
        case 'get_settings':
          return settings;
        case 'update_settings':
          settings = (payload as { s: typeof settings }).s;
          return null;
        case 'get_pricing':
          return PRICING;
        case 'get_session_history':
          return windowOf(data, days(payload), now).events;
        case 'get_session_costs':
          return windowOf(data, days(payload), now).sessionCosts;
        case 'get_compactions':
          return windowOf(data, days(payload), now).compactions;
        case 'get_daily_trends':
          return windowOf(data, days(payload), now).daily;
        case 'get_daily_model_breakdown':
          return windowOf(data, days(payload), now).dailyModels;
        case 'get_model_breakdown':
          return windowOf(data, days(payload), now).models;
        case 'get_project_breakdown':
          return windowOf(data, days(payload), now).projects;
        case 'get_cache_stats':
          return windowOf(data, days(payload), now).cache;
        case 'list_resumable_sessions':
          return data.sessions;
        case 'list_providers':
          return data.providers;
        case 'list_provider_presets':
          return [];
        case 'list_available_terminals':
          return ['terminal_app'];
        case 'get_statusline_bridge_status':
          return [];
        case 'get_warmup_state':
          return { warmup_enabled: false, schedule: { type: 'Off' }, last_warmup_at: null };
        case 'detect_running_claude_code':
          return { cli_processes: 0, vscode_with_extension: [] };
        case 'has_claude_code_creds':
          return true;
        case 'vscode_tab_available':
        case 'get_warmup_consent_granted':
        case 'os_scheduler_is_registered':
          return false;
        case 'get_account_rotation_status':
        case 'get_default_provider':
        case 'resize_window':
        case 'force_refresh':
        case 'check_for_updates_now':
          return null;
        default:
          console.warn(`[preview] unmocked command: ${cmd}`, payload);
          return null;
      }
  };

  /** Report queries, which `?delay` slows down. Startup calls stay instant. */
  const SLOW = /^(get_(session_history|compactions|daily_|model_|project_|cache_|session_costs)|list_resumable_sessions)/;

  mockIPC(
    (cmd, payload) => {
      if (cmd.startsWith('plugin:')) return null;
      const value = answer(cmd, payload);
      if (delay === 0 || !SLOW.test(cmd)) return value;
      return new Promise((resolve) => window.setTimeout(() => resolve(value), delay));
    },
    { shouldMockEvents: true },
  );

  // Imported after the mocks are in place: the store talks to Tauri on load.
  const [{ App }, { useAppStore }] = await Promise.all([import('../App'), import('../lib/store')]);
  useAppStore.setState({ viewMode });

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void start();
