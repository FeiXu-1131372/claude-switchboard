import { motion } from 'framer-motion';
import { invoke } from '@tauri-apps/api/core';
import { CircleArrowUp } from 'lucide-react';
import { useUpdateStore } from '../state/updateStore';

export function UpdateBanner() {
  const status = useUpdateStore((s) => s.status);
  const version = useUpdateStore((s) => s.version);
  const error = useUpdateStore((s) => s.error);

  const showInstall = status === 'ready';
  const showRetry = status === 'failed' && error?.phase === 'install';

  if (!showInstall && !showRetry) return null;

  const handleClick = () => {
    invoke('install_update').catch(() => {
      // Errors arrive via the update://failed event; nothing to do here.
    });
  };

  return (
    <motion.div
      initial={{ y: -36, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      transition={{ type: 'spring', stiffness: 280, damping: 28 }}
      className="flex items-center gap-2 px-3 py-2 border-b border-[color:var(--color-border-subtle)] bg-[color:var(--color-accent-dim)]"
      role="status"
    >
      <CircleArrowUp size={14} className="text-[color:var(--color-accent-text)]" aria-hidden />
      <span className="flex-1 text-xs text-[color:var(--color-text)] tracking-tight">
        {showInstall ? `Update ready · v${version}` : 'Install failed'}
      </span>
      <button
        type="button"
        onClick={handleClick}
        className="text-xs text-[color:var(--color-accent-text)] px-2 py-1 rounded-[var(--radius-sm)] hover:bg-[color:var(--color-accent-muted)] transition-colors"
      >
        {showInstall ? 'Install & restart' : 'Retry'}
      </button>
    </motion.div>
  );
}
