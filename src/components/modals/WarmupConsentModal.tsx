interface Props {
  onAccept: () => void;
  onDismiss: () => void;
}

export function WarmupConsentModal({ onAccept, onDismiss }: Props) {
  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3"
    >
      <div className="w-full max-w-[320px] max-h-full overflow-y-auto rounded-lg border border-orange-500/12 bg-neutral-900/95 backdrop-blur p-3.5 text-[12px] text-neutral-100 shadow-2xl">
        <h2 className="text-[13px] font-semibold mb-2 leading-tight">
          Warm-up sends messages on your behalf
        </h2>
        <p className="text-neutral-300 leading-snug mb-2">
          Switchboard can send a tiny message (1 token, on Haiku) to{" "}
          <code>api.anthropic.com</code> using this account's credentials,
          whenever you trigger it manually or a schedule fires.
        </p>
        <p className="text-neutral-300 leading-snug mb-2">
          Same API surface Claude Code uses. Cost: rounding-error against
          your subscription. Effect: starts the 5-hour window deliberately.
        </p>
        <p className="text-neutral-400 leading-snug text-[11px] mb-3">
          You can disable per-account, or revoke globally from Settings.
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onDismiss}
            className="px-3 py-1 rounded-md bg-neutral-700/40 hover:bg-neutral-700/60 text-neutral-200 text-[11px] font-medium transition-colors"
          >
            Don't enable
          </button>
          <button
            type="button"
            onClick={onAccept}
            className="px-3 py-1 rounded-md bg-teal-500/20 hover:bg-teal-500/30 text-teal-100 text-[11px] font-medium transition-colors"
          >
            Enable warm-up
          </button>
        </div>
      </div>
    </div>
  );
}
