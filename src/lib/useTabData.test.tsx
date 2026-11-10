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
});
