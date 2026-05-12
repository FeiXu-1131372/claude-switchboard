import type { PointerEvent as ReactPointerEvent } from 'react';
import { getCurrentWindow, LogicalPosition } from '@tauri-apps/api/window';
import { emit } from '@tauri-apps/api/event';

/**
 * Manual window-drag implementation. Tauri's `data-tauri-drag-region` and
 * `startDragging()` both rely on the underlying NSWindow accepting a drag via
 * `performWindowDragWithEvent:`, which is unreliable on macOS for transparent
 * + alwaysOnTop popover windows — the IPC succeeds but the window doesn't
 * actually move. We track pointer movement ourselves and reposition via
 * `setPosition`.
 *
 * `pointermove`/`pointerup` are attached to `window` rather than the drag
 * host so the drag survives the cursor outrunning the (async) window move
 * and momentarily leaving the host element.
 */
export async function handleDragStart(e: ReactPointerEvent<HTMLElement>) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement;
  if (target.closest('button, input, a, select, textarea')) return;

  // On Windows, startDragging() must be called BEFORE e.preventDefault(),
  // otherwise the OS never receives the clean mousedown event it needs to
  // initiate a native window drag. The manual setPosition path used on macOS
  // requires preventDefault() to suppress text-selection, but Windows does not.
  if (navigator.userAgent.includes('Windows')) {
    try {
      const win = getCurrentWindow();
      win.startDragging().catch(() => {});
    } catch {
      // outside Tauri — no-op
    }
    return;
  }

  e.preventDefault();

  const pointerId = e.pointerId;

  let win: ReturnType<typeof getCurrentWindow>;
  let startWinLogical: { x: number; y: number };
  try {
    win = getCurrentWindow();
    const pos = await win.outerPosition();
    const scaleFactor = await win.scaleFactor();
    startWinLogical = { x: pos.x / scaleFactor, y: pos.y / scaleFactor };
  } catch {
    // outside Tauri (e.g. demo HTML on localhost) — silently no-op.
    return;
  }

  const startCursor = { x: e.screenX, y: e.screenY };

  const onMove = (ev: PointerEvent) => {
    if (ev.pointerId !== pointerId) return;
    const x = startWinLogical.x + (ev.screenX - startCursor.x);
    const y = startWinLogical.y + (ev.screenY - startCursor.y);
    win.setPosition(new LogicalPosition(x, y)).catch(() => { });
  };

  const cleanup = () => {
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', cleanup);
    window.removeEventListener('pointercancel', cleanup);
  };

  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', cleanup);
  window.addEventListener('pointercancel', cleanup);
}

export async function closeWindow() {
  try {
    const win = getCurrentWindow();
    await win.hide();
    await emit('popover_hidden');
  } catch {
    // No-op outside Tauri or if the window's already hidden.
  }
}
