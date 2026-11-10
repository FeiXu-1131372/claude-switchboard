import { useCallback, useEffect, useState } from 'react';

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

  useEffect(() => {
    let cancelled = false;
    setState((prev) => {
      const base = prev.key === cacheKey ? prev : startingPoint<T>(cacheKey);
      if (base === prev && prev.error === null) return prev;
      return { ...base, error: null, settled: base.data !== null };
    });
    loader()
      .then((data) => {
        if (cancelled) return;
        if (cacheKey !== undefined) cache.set(cacheKey, data);
        setState({ key: cacheKey, data, error: null, settled: true });
      })
      .catch((e) => {
        if (cancelled) return;
        setState((prev) => ({
          key: cacheKey,
          data: prev.key === cacheKey ? prev.data : startingPoint<T>(cacheKey).data,
          error: toMessage(e),
          settled: true,
        }));
      });
    return () => {
      cancelled = true;
    };
    // The loader closure is recomputed on every render; we deliberately
    // re-run only on explicit reload() (tick), a new key, or an external trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick, cacheKey, ...triggers]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { data: view.data, error: view.error, loading: !view.settled, reload };
}
