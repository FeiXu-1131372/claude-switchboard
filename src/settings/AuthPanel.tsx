import { useState, useEffect } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { listen } from '@tauri-apps/api/event';
import { motion } from 'framer-motion';
import { Card } from '../components/ui/Card';
import { Button } from '../components/ui/Button';
import { IconButton } from '../components/ui/IconButton';
import { fadeIn } from '../lib/motion';
import { IconAuth, IconRefresh, IconTimer, ExternalLink, X, ChevronRight } from '../lib/icons';
import { ipc } from '../lib/ipc';
import { useAppStore } from '../lib/store';
import { handleDragStart, closeWindow } from '../lib/window-chrome';

type Step = 'choose' | 'waiting';

function toMessage(e: unknown, fallback: string): string {
  if (e instanceof Error && e.message) return e.message;
  if (typeof e === 'string' && e.length > 0) return e;
  if (e && typeof e === 'object' && 'message' in e && typeof (e as { message: unknown }).message === 'string') {
    return (e as { message: string }).message;
  }
  return fallback;
}

interface Props {
  onBack?: () => void;
}

export function AuthPanel({ onBack }: Props) {
  const [step, setStep] = useState<Step>('choose');
  const [error, setError] = useState<string | null>(null);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);

  useEffect(() => {
    if (step !== 'waiting') return;

    const unlistenComplete = listen<number>('oauth_complete', async () => {
      await refreshAccounts();
      setStep('choose');
      setError(null);
    });

    const unlistenError = listen<string>('oauth_error', (event) => {
      setError(event.payload ?? 'Sign-in failed. Try again.');
      setStep('choose');
    });

    return () => {
      unlistenComplete.then((f) => f());
      unlistenError.then((f) => f());
    };
  }, [step, refreshAccounts]);

  async function startOauth(longLived: boolean = false) {
    setError(null);
    let url: string;
    try {
      url = await ipc.startOauthFlow(longLived);
    } catch (e) {
      setError(toMessage(e, 'Failed to start sign-in.'));
      return;
    }
    try {
      await openUrl(url);
    } catch (e) {
      setError(
        `Could not open your browser (${toMessage(e, 'unknown error')}). Open this URL manually: ${url}`,
      );
    }
    setStep('waiting');
  }

  async function useLocal() {
    setError(null);
    try {
      await ipc.addAccountFromClaudeCode();
      await refreshAccounts();
    } catch (e) {
      setError(toMessage(e, "Couldn't import the upstream login."));
    }
  }

  return (
    <div className="relative flex flex-col h-full">
      <div
        onPointerDown={handleDragStart}
        className={`flex items-center ${onBack ? 'justify-between' : 'justify-end'} gap-[var(--space-sm)] px-[var(--popover-pad)] pt-[var(--space-md)] pb-[var(--space-sm)] cursor-default select-none`}
      >
        {onBack && (
          <button
            type="button"
            onClick={onBack}
            className="inline-flex items-center gap-[var(--space-2xs)] text-[length:var(--text-label)] text-[color:var(--color-text-secondary)] tracking-[var(--tracking-label)] uppercase transition-colors duration-[var(--duration-fast)] hover:text-[color:var(--color-text)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-2 rounded"
          >
            <ChevronRight size={11} className="rotate-180" />
            Back
          </button>
        )}
        <IconButton label="Close" onClick={closeWindow}>
          <X size={13} />
        </IconButton>
      </div>
      <div className="flex items-center justify-center flex-1 min-h-0 overflow-y-auto px-[var(--space-2xl)] pb-[var(--space-2xl)]">
        <motion.div
          className="flex flex-col gap-[var(--space-xl)] max-w-[280px]"
          variants={fadeIn}
          initial="hidden"
          animate="visible"
        >
          {/* Icon */}
          <div className="flex justify-center">
            <div className="w-[48px] h-[48px] rounded-[var(--radius-lg)] bg-[var(--color-accent-dim)] flex items-center justify-center">
              <IconAuth size={24} className="text-[color:var(--color-accent)]" />
            </div>
          </div>

          {/* Title */}
          <div className="text-center flex flex-col gap-[var(--space-xs)]">
            <h1 className="text-[length:var(--text-title)] font-[var(--weight-semibold)] text-[color:var(--color-text)]">
              Connect to Claude
            </h1>
            <p className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)] leading-[var(--leading-label)]">
              {step === 'waiting'
                ? 'Complete authorization in your browser, then return here.'
                : 'Choose how to authenticate. Your credentials stay on this device.'}
            </p>
          </div>

          {step === 'choose' && (
            <div className="flex flex-col gap-[var(--space-sm)]">
              <Card hover className="p-[var(--space-md)]">
                <button
                  type="button"
                  onClick={() => startOauth(false)}
                  className="w-full flex items-center gap-[var(--space-sm)] text-left"
                >
                  <div className="w-[32px] h-[32px] rounded-[var(--radius-sm)] bg-[var(--color-accent-dim)] flex items-center justify-center shrink-0">
                    <ExternalLink size={14} className="text-[color:var(--color-accent)]" />
                  </div>
                  <div className="flex flex-col gap-[2px] flex-1">
                    <span className="text-[length:var(--text-body)] font-[var(--weight-medium)] text-[color:var(--color-text)]">
                      Sign in with Claude
                    </span>
                    <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      Opens browser — refreshes automatically
                    </span>
                  </div>
                </button>
              </Card>

              <Card hover className="p-[var(--space-md)]">
                <button
                  type="button"
                  onClick={() => startOauth(true)}
                  className="w-full flex items-center gap-[var(--space-sm)] text-left"
                >
                  <div className="w-[32px] h-[32px] rounded-[var(--radius-sm)] bg-[var(--color-accent-dim)] flex items-center justify-center shrink-0">
                    <IconTimer size={14} className="text-[color:var(--color-accent)]" />
                  </div>
                  <div className="flex flex-col gap-[2px] flex-1">
                    <span className="text-[length:var(--text-body)] font-[var(--weight-medium)] text-[color:var(--color-text)]">
                      Request long-lived token
                    </span>
                    <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      Opens browser — 1-year token, no refresh needed
                    </span>
                  </div>
                </button>
              </Card>

              <Card hover className="p-[var(--space-md)]">
                <button
                  type="button"
                  onClick={useLocal}
                  className="w-full flex items-center gap-[var(--space-sm)] text-left"
                >
                  <div className="w-[32px] h-[32px] rounded-[var(--radius-sm)] bg-[var(--color-track)] flex items-center justify-center shrink-0">
                    <IconAuth size={14} className="text-[color:var(--color-text-secondary)]" />
                  </div>
                  <div className="flex flex-col gap-[2px] flex-1">
                    <span className="text-[length:var(--text-body)] font-[var(--weight-medium)] text-[color:var(--color-text)]">
                      Use upstream's current login
                    </span>
                    <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
                      Imports the account you're signed into in the CLI
                    </span>
                  </div>
                </button>
              </Card>
            </div>
          )}

          {step === 'waiting' && (
            <div className="flex flex-col items-center gap-[var(--space-md)]">
              <div className="flex items-center gap-[var(--space-sm)]">
                <IconRefresh size={14} className="text-[color:var(--color-accent)] animate-spin" />
                <span className="text-[length:var(--text-label)] text-[color:var(--color-text-muted)]">
                  Waiting for browser…
                </span>
              </div>
              <Button variant="ghost" size="sm" onClick={() => { setStep('choose'); setError(null); }}>
                Cancel
              </Button>
            </div>
          )}

          {error && (
            <p className="text-[length:var(--text-label)] text-[color:var(--color-danger)]">
              {error}
            </p>
          )}

          <p className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)] text-center">
            Credentials are stored in your OS keychain and never leave this device.
          </p>
        </motion.div>
      </div>
    </div>
  );
}
