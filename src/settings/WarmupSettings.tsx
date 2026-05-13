interface Props {
  consentGranted: boolean;
  osSchedulerRegistered: boolean;
  onRevoke: () => void;
  onRegisterOs: () => void;
  onUnregisterOs: () => void;
}

export function WarmupSettings({
  consentGranted,
  osSchedulerRegistered,
  onRevoke,
  onRegisterOs,
  onUnregisterOs,
}: Props) {
  return (
    <div className="space-y-3 text-[12px]">
      <div className="flex items-center justify-between">
        <span className="text-neutral-300">
          Global consent: {consentGranted ? "granted" : "not granted"}
        </span>
        {consentGranted && (
          <button
            type="button"
            onClick={onRevoke}
            className="px-2 py-0.5 rounded bg-[var(--color-danger-dim)] hover:bg-[var(--color-danger)] hover:text-white text-[color:var(--color-danger)] text-[11px]"
          >
            Revoke
          </button>
        )}
      </div>
      <div className="flex items-center justify-between">
        <span className="text-neutral-300">
          OS-level scheduler:{" "}
          {osSchedulerRegistered ? "registered" : "not registered"}
        </span>
        {osSchedulerRegistered ? (
          <button
            type="button"
            onClick={onUnregisterOs}
            className="px-2 py-0.5 rounded bg-[var(--color-bg-surface-hover)] hover:bg-[var(--color-border-hover)] text-[color:var(--color-text)] text-[11px]"
          >
            Unregister
          </button>
        ) : (
          <button
            type="button"
            onClick={onRegisterOs}
            className="px-2 py-0.5 rounded bg-[var(--color-teal-dim)] hover:bg-teal-500/25 text-[color:var(--color-teal)] text-[11px]"
          >
            Register
          </button>
        )}
      </div>
    </div>
  );
}
