import { Zap } from "lucide-react";

interface Props {
  enabled: boolean;
  onClick: () => void;
}

export function WarmupNowButton({ enabled, onClick }: Props) {
  return (
    <button
      type="button"
      disabled={!enabled}
      onClick={onClick}
      className={[
        "flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium",
        enabled
          ? "bg-teal-500/15 hover:bg-teal-500/25 text-teal-200"
          : "bg-neutral-800/30 text-neutral-500 cursor-not-allowed",
      ].join(" ")}
    >
      <Zap className="w-3 h-3" />
      Warm up now
    </button>
  );
}
