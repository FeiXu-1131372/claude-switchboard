interface Props {
  enabled: boolean;
  onToggle: (next: boolean) => void;
}

export function WarmupToggle({ enabled, onToggle }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={enabled}
      onClick={() => onToggle(!enabled)}
      className={[
        "relative h-4 w-7 rounded-full transition-colors",
        enabled ? "bg-teal-500/60" : "bg-neutral-600/40",
      ].join(" ")}
    >
      <span
        className={[
          "absolute top-0.5 h-3 w-3 rounded-full bg-white transition-all",
          enabled ? "left-3.5" : "left-0.5",
        ].join(" ")}
      />
    </button>
  );
}
