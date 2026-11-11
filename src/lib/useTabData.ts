import { useCallback, useEffect, useRef, useState } from 'react';

function toMessage(e: unknown): string {
  if (e instanceof Error && e.message) return e.message;
  if (typeof e === 'string' && e.length > 0) return e;
  if (e && typeof e === 'object' && 'message' in e && typeof (e as { message: unknown }).message === 'string') {
    return (e as { message: string }).message;
  }
  return 'Unknown error';
}

export interface TabDataState<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
}

/** The last answer for each `cacheKey`, kept for as long as the app runs.
 * Tabs unmount when you leave them; this is what lets one come back already
 * showing its data instead of a loading state. */
const cache = new Map<string, unknown>();

/** Forget every cached answer. For tests. */
export function clearTabDataCache() {
  cache.clear();
}

interface Loaded<T> {
  key: string | undefined;
  data: T | null;
  error: string | null;
  /** False only while there is nothing to show yet. */
  settled: boolean;
}

function startingPoint<T>(key: string | undefined): Loaded<T> {
  const known = key !== undefined && cache.has(key);
  return { key, data: known ? (cache.get(key) as T) : null, error: null, settled: known };
}

/**
 * Loads tab data with consistent loading/error/reload semantics. The loader
 * runs on mount, on explicit reload(), and whenever any value in `triggers`
 * changes — pass a session-data-version counter here so the tab auto-refreshes
 * as new data arrives in the background. A refresh never brings the loading
 * state back: the data on screen stays until its replacement arrives.
 * Triggers that change while a load is still running do not start another:
 * that load lands, and one catch-up load follows.
 *
 * `cacheKey` names the question being asked (`'models:30'`). With one, the
 * last answer is shown at once on the next mount while a fresh one loads, and
 * a change of key is a new question: the old answer is never shown under it.
 */
export function useTabData<T>(
  loader: () => Promise<T>,
  triggers: ReadonlyArray<unknown> = [],
  cacheKey?: string,
): TabDataState<T> {
  const [state, setState] = useState<Loaded<T>>(() => startingPoint<T>(cacheKey));
  const [tick, setTick] = useState(0);
  // Derived during render, not in the effect, so a key change cannot paint
  // one frame of the previous key's data.
  const view = state.key === cacheKey ? state : startingPoint<T>(cacheKey);

  // The loader closure is new on every render. The run below always calls
  // the latest one, including for a catch-up load started from a promise.
  const latest = useRef({ loader, cacheKey });
  useEffect(() => {
    latest.current = { loader, cacheKey };
  });

  // One load at a time per question. `id` names the load whose answer counts:
  // a new key, reload() or leaving the tab moves it on, and an answer that
  // comes back under an old id is dropped before it reaches the cache.
  const run = useRef({ id: 0, busy: false, again: false, key: cacheKey, tick });

  const start = useCallback(() => {
    const r = run.current;
    const id = ++r.id;
    const key = latest.current.cacheKey;
    r.busy = true;
    r.again = false;
    setState((prev) => {
      const base = prev.key === key ? prev : startingPoint<T>(key);
      if (base === prev && prev.error === null) return prev;
      return { ...base, error: null, settled: base.data !== null };
    });
    latest.current
      .loader()
      .then((data) => {
        if (id !== r.id) return;
        if (key !== undefined) cache.set(key, data);
        setState({ key, data, error: null, settled: true });
      })
      .catch((e) => {
        if (id !== r.id) return;
        setState((prev) => ({
          key,
          data: prev.key === key ? prev.data : startingPoint<T>(key).data,
          error: toMessage(e),
          settled: true,
        }));
      })
      .finally(() => {
        if (id !== r.id) return;
        r.busy = false;
        // Ticks that arrived meanwhile are answered by one catch-up load.
        if (r.again) start();
      });
  }, []);

  useEffect(() => {
    const r = run.current;
    const onlyATrigger = r.key === cacheKey && r.tick === tick;
    r.key = cacheKey;
    r.tick = tick;
    // New data arriving while a load is already running: starting another
    // for every tick would throw away each answer just before it lands, so
    // during a burst nothing would ever be shown, and the scans would queue
    // up behind each other. Let this one land, then load once more.
    if (onlyATrigger && r.busy) {
      r.again = true;
      return;
    }
    start();
    // Re-run only on explicit reload() (tick), a new key, or an external
    // trigger; the loader itself is read from `latest`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, cacheKey, ...triggers]);

  // Leaving the tab: whatever is in flight no longer counts, and no catch-up
  // load starts.
  useEffect(() => {
    const r = run.current;
    return () => {
      r.id++;
      r.busy = false;
      r.again = false;
    };
  }, []);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { data: view.data, error: view.error, loading: !view.settled, reload };
}
