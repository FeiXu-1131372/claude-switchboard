import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { clearTabDataCache, useTabData } from './useTabData';

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => clearTabDataCache());

describe('useTabData', () => {
  it('loads on mount', async () => {
    const { result } = renderHook(() => useTabData(() => Promise.resolve('A')));
    expect(result.current.loading).toBe(true);
    expect(result.current.data).toBeNull();
    await waitFor(() => expect(result.current.data).toBe('A'));
    expect(result.current.loading).toBe(false);
  });

  it('shows cached data at once on remount and still refreshes', async () => {
    const first = renderHook(() => useTabData(() => Promise.resolve('A'), [], 'k'));
    await waitFor(() => expect(first.result.current.data).toBe('A'));
    first.unmount();

    const next = deferred<string>();
    const second = renderHook(() => useTabData(() => next.promise, [], 'k'));
    // First render: yesterday's answer, no loading state.
    expect(second.result.current.data).toBe('A');
    expect(second.result.current.loading).toBe(false);

    await act(async () => next.resolve('B'));
    expect(second.result.current.data).toBe('B');
  });

  it('does not share data between keys', async () => {
    const first = renderHook(() => useTabData(() => Promise.resolve('A'), [], 'k1'));
    await waitFor(() => expect(first.result.current.data).toBe('A'));
    first.unmount();
    const second = renderHook(() => useTabData(() => new Promise<string>(() => {}), [], 'k2'));
    expect(second.result.current.data).toBeNull();
    expect(second.result.current.loading).toBe(true);
  });

  it('a trigger change refetches without setting loading', async () => {
    const loader = vi.fn().mockResolvedValueOnce('A');
    const { result, rerender } = renderHook(({ v }) => useTabData(loader, [v], 'k'), {
      initialProps: { v: 0 },
    });
    await waitFor(() => expect(result.current.data).toBe('A'));

    const next = deferred<string>();
    loader.mockReturnValueOnce(next.promise);
    rerender({ v: 1 });
    expect(loader).toHaveBeenCalledTimes(2);
    // While the refetch is in flight the old data stays and nothing flashes.
    expect(result.current.loading).toBe(false);
    expect(result.current.data).toBe('A');

    await act(async () => next.resolve('B'));
    expect(result.current.data).toBe('B');
  });

  it('a new key never shows the old key’s data', async () => {
    const loader = vi.fn().mockResolvedValueOnce('thirty days');
    const { result, rerender } = renderHook(({ k }) => useTabData(loader, [], k), {
      initialProps: { k: 'models:30' },
    });
    await waitFor(() => expect(result.current.data).toBe('thirty days'));

    const next = deferred<string>();
    loader.mockReturnValueOnce(next.promise);
    rerender({ k: 'models:90' });
    expect(result.current.data).toBeNull();
    expect(result.current.loading).toBe(true);

    await act(async () => next.resolve('ninety days'));
    expect(result.current.data).toBe('ninety days');
    expect(result.current.loading).toBe(false);
  });

  it('reports an error and recovers on reload', async () => {
    const loader = vi.fn().mockRejectedValueOnce(new Error('boom'));
    const { result } = renderHook(() => useTabData(loader, [], 'k'));
    await waitFor(() => expect(result.current.error).toBe('boom'));
    expect(result.current.loading).toBe(false);

    loader.mockResolvedValueOnce('A');
    act(() => result.current.reload());
    await waitFor(() => expect(result.current.data).toBe('A'));
    expect(result.current.error).toBeNull();
  });

  it('keeps showing the last good data when a refresh fails', async () => {
    const loader = vi.fn().mockResolvedValueOnce('A');
    const { result, rerender } = renderHook(({ v }) => useTabData(loader, [v], 'k'), {
      initialProps: { v: 0 },
    });
    await waitFor(() => expect(result.current.data).toBe('A'));
    loader.mockRejectedValueOnce(new Error('offline'));
    rerender({ v: 1 });
    await waitFor(() => expect(result.current.error).toBe('offline'));
    expect(result.current.data).toBe('A');
  });

  describe('refreshes that arrive faster than a load takes', () => {
    /** A loader whose every call is answered by hand, in order. */
    function manualLoader() {
      const calls: ReturnType<typeof deferred<string>>[] = [];
      const loader = vi.fn(() => {
        const next = deferred<string>();
        calls.push(next);
        return next.promise;
      });
      return { loader, calls };
    }

    it('lets the load in flight land, then runs one more, however many ticks arrived', async () => {
      const { loader, calls } = manualLoader();
      const { result, rerender } = renderHook(({ v }) => useTabData(loader, [v], 'k'), {
        initialProps: { v: 0 },
      });
      expect(loader).toHaveBeenCalledTimes(1);

      // Three ticks while the first load is still running: no new load each.
      rerender({ v: 1 });
      rerender({ v: 2 });
      rerender({ v: 3 });
      expect(loader).toHaveBeenCalledTimes(1);

      // The first answer is shown, not thrown away...
      await act(async () => calls[0].resolve('first'));
      expect(result.current.data).toBe('first');
      // ...and exactly one catch-up load follows.
      expect(loader).toHaveBeenCalledTimes(2);

      await act(async () => calls[1].resolve('second'));
      expect(result.current.data).toBe('second');
      expect(loader).toHaveBeenCalledTimes(2);
    });

    it('runs the catch-up load after a load that failed', async () => {
      const { loader, calls } = manualLoader();
      const { result, rerender } = renderHook(({ v }) => useTabData(loader, [v], 'k'), {
        initialProps: { v: 0 },
      });
      rerender({ v: 1 });
      await act(async () => calls[0].reject(new Error('busy')));
      expect(loader).toHaveBeenCalledTimes(2);
      await act(async () => calls[1].resolve('ok'));
      expect(result.current.data).toBe('ok');
      expect(result.current.error).toBeNull();
    });

    it('a tick with nothing in flight loads straight away', async () => {
      const { loader, calls } = manualLoader();
      const { rerender } = renderHook(({ v }) => useTabData(loader, [v], 'k'), { initialProps: { v: 0 } });
      await act(async () => calls[0].resolve('first'));
      rerender({ v: 1 });
      expect(loader).toHaveBeenCalledTimes(2);
    });

    it('a new key does not wait for the old key’s load, and drops its answer', async () => {
      const { loader, calls } = manualLoader();
      const { result, rerender } = renderHook(({ k }) => useTabData(loader, [], k), {
        initialProps: { k: 'a' },
      });
      rerender({ k: 'b' });
      expect(loader).toHaveBeenCalledTimes(2);

      await act(async () => calls[0].resolve('for a'));
      expect(result.current.data).toBeNull();
      await act(async () => calls[1].resolve('for b'));
      expect(result.current.data).toBe('for b');
      expect(loader).toHaveBeenCalledTimes(2);
    });

    it('reload() does not wait for the load in flight', async () => {
      const { loader, calls } = manualLoader();
      const { result } = renderHook(() => useTabData(loader, [], 'k'));
      act(() => result.current.reload());
      expect(loader).toHaveBeenCalledTimes(2);
      // The older answer arrives late and is ignored.
      await act(async () => calls[1].resolve('newer'));
      await act(async () => calls[0].resolve('older'));
      expect(result.current.data).toBe('newer');
    });

    it('starts nothing after the tab is left', async () => {
      const { loader, calls } = manualLoader();
      const { rerender, unmount } = renderHook(({ v }) => useTabData(loader, [v], 'k'), {
        initialProps: { v: 0 },
      });
      rerender({ v: 1 });
      unmount();
      await act(async () => calls[0].resolve('late'));
      expect(loader).toHaveBeenCalledTimes(1);
    });
  });
});
