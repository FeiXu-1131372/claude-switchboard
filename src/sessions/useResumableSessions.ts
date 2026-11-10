import type { SessionSummary } from '../lib/generated/bindings';
import { ipc } from '../lib/ipc';
import { useTabData } from '../lib/useTabData';

/** Cache key for the transcript scan, shared by every view that lists
 * sessions so the second one to open does not wait for it again. */
export const RESUMABLE_SESSIONS_KEY = 'resumable-sessions';

const NONE: SessionSummary[] = [];

export function useResumableSessions() {
  const { data, loading, error, reload } = useTabData(
    () => ipc.listResumableSessions(),
    [],
    RESUMABLE_SESSIONS_KEY,
  );
  return { sessions: data ?? NONE, loading, error, reload };
}
