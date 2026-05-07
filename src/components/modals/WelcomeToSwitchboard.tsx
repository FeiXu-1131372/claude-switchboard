import { CheckCircle2 } from "lucide-react";
import { LEGACY_PRODUCT_NAME, PRODUCT_NAME } from "@/lib/branding";

export interface MigrationOutcome {
  legacy_data_dir_found: boolean;
  files_copied: number;
  legacy_process_quit: boolean;
  legacy_autostart_removed: boolean;
}

interface Props {
  outcome: MigrationOutcome;
  onClose: () => void;
}

export function WelcomeToSwitchboard({ outcome, onClose }: Props) {
  if (!outcome.legacy_data_dir_found) return null;

  return (
    <div
      role="dialog"
      aria-modal="true"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3"
    >
      <div className="w-full max-w-[320px] max-h-full overflow-y-auto rounded-lg border border-orange-500/12 bg-neutral-900/95 backdrop-blur p-3.5 text-[12px] text-neutral-100 shadow-2xl">
        <div className="flex items-start gap-2 mb-2">
          <CheckCircle2 className="w-4 h-4 text-teal-400 shrink-0 mt-0.5" />
          <div className="min-w-0">
            <h2 className="text-[13px] font-semibold leading-tight">
              Welcome to {PRODUCT_NAME}
            </h2>
            <p className="text-neutral-300 mt-1 leading-snug">
              {LEGACY_PRODUCT_NAME} is now {PRODUCT_NAME}. Your data was
              migrated automatically.
            </p>
          </div>
        </div>

        <ul className="space-y-0.5 text-neutral-300 pl-4 list-disc list-outside marker:text-neutral-500">
          <li>{outcome.files_copied} files migrated</li>
          {outcome.legacy_process_quit && <li>Previous app closed</li>}
          {outcome.legacy_autostart_removed && <li>Legacy autostart cleared</li>}
        </ul>

        <p className="text-neutral-400 mt-2 text-[11px] leading-snug">
          Your old install is preserved. After a few weeks of stable use you'll
          see a "tidy old data" option in Settings.
        </p>

        <div className="mt-3 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1 rounded-md bg-teal-500/15 hover:bg-teal-500/25 text-teal-200 text-[11px] font-medium transition-colors"
          >
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
