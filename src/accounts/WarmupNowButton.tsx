import { Check, Zap } from "lucide-react";
import { IconRefresh, AlertTriangle } from "../lib/icons";

export type WarmupNowStatus = "idle" | "success" | "error";

interface Props {
  enabled: boolean;
  onClick: () => void;
  busy?: boolean;
  status?: WarmupNowStatus;
}

export function WarmupNowButton({ enabled, onClick, busy = false, status = "idle" }: Props) {
  const disabled = !enabled || busy;
  const label = busy
    ? "Warming up…"
    : status === "success"
      ? "Warmed up"
      : status === "error"
        ? "Try again"
        : "Warm up now";
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={[
        "flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium",
        "transition-colors duration-[var(--duration-fast)]",
        disabled && !busy
          ? "bg-neutral-800/30 text-neutral-500 cursor-not-allowed"
          : status === "success"
            ? "bg-[var(--color-safe-dim)] text-[color:var(--color-safe)]"
            : status === "error"
              ? "bg-[var(--color-danger-dim)] text-[color:var(--color-danger)] hover:bg-[var(--color-danger)] hover:text-white"
              : busy
                ? "bg-teal-500/15 text-teal-200 cursor-progress"
                : "bg-teal-500/15 hover:bg-teal-500/25 text-teal-200",
      ].join(" ")}
    >
      {busy ? (
        <IconRefresh className="w-3 h-3 animate-spin" />
      ) : status === "success" ? (
        <Check className="w-3 h-3" />
      ) : status === "error" ? (
        <AlertTriangle className="w-3 h-3" />
      ) : (
        <Zap className="w-3 h-3" />
      )}
      {label}
    </button>
  );
}
