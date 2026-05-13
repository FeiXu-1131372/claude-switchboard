import { ModalShell } from './ModalShell';

interface Props {
  onAccept: () => void;
  onDismiss: () => void;
}

export function WarmupConsentModal({ onAccept, onDismiss }: Props) {
  return (
    <ModalShell id="warmup-consent" onDismiss={onDismiss} size="sm" dismissable={false}>
      <div className="p-[var(--space-md)] text-[length:var(--text-label)] text-[color:var(--color-text)]">
        <h2 className="text-[length:var(--text-body)] font-[var(--weight-semibold)] mb-[var(--space-xs)] leading-tight">
          Warm-up sends messages on your behalf
        </h2>
        <p className="text-[color:var(--color-text-secondary)] leading-snug mb-[var(--space-xs)]">
          Switchboard can send a tiny message (1 token, on Haiku) to{' '}
          <code className="mono">api.anthropic.com</code> using this account's credentials,
          whenever you trigger it manually or a schedule fires.
        </p>
        <p className="text-[color:var(--color-text-secondary)] leading-snug mb-[var(--space-xs)]">
          Same API surface Claude Code uses. Cost: rounding-error against your
          subscription. Effect: starts the 5-hour window deliberately.
        </p>
        <p className="text-[color:var(--color-text-muted)] leading-snug text-[length:var(--text-micro)] mb-[var(--space-md)]">
          You can disable per-account, or revoke globally from Settings.
        </p>
        <div className="flex justify-end gap-[var(--space-xs)]">
          <button
            type="button"
            onClick={onDismiss}
            className="px-[var(--space-sm)] py-[var(--space-2xs)] rounded-[var(--radius-sm)] bg-[var(--color-track)] hover:bg-[var(--color-bg-card-hover)] text-[color:var(--color-text-secondary)] text-[length:var(--text-micro)] font-[var(--weight-medium)] transition-colors"
          >
            Don't enable
          </button>
          <button
            type="button"
            onClick={onAccept}
            className="px-[var(--space-sm)] py-[var(--space-2xs)] rounded-[var(--radius-sm)] bg-[var(--color-accent-dim)] hover:bg-[var(--color-accent-muted)] text-[color:var(--color-accent)] text-[length:var(--text-micro)] font-[var(--weight-medium)] transition-colors"
          >
            Enable warm-up
          </button>
        </div>
      </div>
    </ModalShell>
  );
}
