import { describe, it, expect, beforeEach } from 'vitest';
import { useAppStore } from '../store';

describe('modalStack', () => {
  beforeEach(() => {
    // Reset stack between tests
    useAppStore.getState().resetModalStack();
  });

  it('starts empty', () => {
    expect(useAppStore.getState().modalStack).toEqual([]);
  });

  it('push appends an id', () => {
    useAppStore.getState().pushModal('a');
    expect(useAppStore.getState().modalStack).toEqual(['a']);
  });

  it('push of multiple ids preserves order', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('b');
    expect(useAppStore.getState().modalStack).toEqual(['a', 'b']);
  });

  it('pop removes the matching id regardless of position', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('b');
    useAppStore.getState().popModal('a');
    expect(useAppStore.getState().modalStack).toEqual(['b']);
  });

  it('pop of unknown id is a no-op', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().popModal('zzz');
    expect(useAppStore.getState().modalStack).toEqual(['a']);
  });

  it('isTopmost returns true for last id', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('b');
    expect(useAppStore.getState().isTopmost('b')).toBe(true);
    expect(useAppStore.getState().isTopmost('a')).toBe(false);
  });

  it('push of same id is idempotent (no-op on duplicate)', () => {
    useAppStore.getState().pushModal('a');
    useAppStore.getState().pushModal('a');
    expect(useAppStore.getState().modalStack).toEqual(['a']);
  });

  it('isTopmost returns false on empty stack', () => {
    expect(useAppStore.getState().isTopmost('a')).toBe(false);
  });
});
