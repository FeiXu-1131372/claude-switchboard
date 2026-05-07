import { AlertTriangle, ArrowRight } from "lucide-react";

interface Props {
  registered: boolean;
  onEnable: () => void;
}

export function OsSchedulerBanner({ registered, onEnable }: Props) {
  if (registered) return null;
  return (
    <div className="flex items-start gap-2 px-3 py-2 border-b border-amber-500/15 bg-amber-500/8 text-[12px] leading-snug">
      <AlertTriangle className="w-4 h-4 text-amber-400 flex-shrink-0 mt-0.5" />
      <div className="flex-1">
        <div className="text-amber-200/90">
          Schedules only fire while the app is open.
        </div>
        <button
          type="button"
          onClick={onEnable}
          className="mt-0.5 text-amber-200/80 underline decoration-dotted underline-offset-2 inline-flex items-center gap-0.5 hover:text-amber-100"
        >
          Enable OS-level scheduling
          <ArrowRight className="w-3 h-3" />
        </button>
      </div>
    </div>
  );
}
