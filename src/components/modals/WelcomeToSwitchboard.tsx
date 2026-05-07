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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4"
    >
      <div className="max-w-sm w-full rounded-xl border border-orange-500/12 bg-neutral-900/95 backdrop-blur p-5 text-[13px] text-neutral-100 shadow-2xl">
        <div className="flex items-start gap-3 mb-3">
          <CheckCircle2 className="w-5 h-5 text-teal-400 flex-shrink-0 mt-0.5" />
          <div>
            <h2 className="text-base font-semibold">
              Welcome to {PRODUCT_NAME}
            </h2>
            <p className="text-neutral-300 mt-1 leading-snug">
              {LEGACY_PRODUCT_NAME} is now {PRODUCT_NAME} — same app, broader
              scope. Your data has been migrated automatically.
            </p>
          </div>
        </div>

        <ul className="space-y-1 text-neutral-300 ml-8 list-disc list-inside">
          <li>{outcome.files_copied} files migrated (usage history, accounts, settings)</li>
          {outcome.legacy_process_quit && (
            <li>The previous {LEGACY_PRODUCT_NAME} app was closed</li>
          )}
          {outcome.legacy_autostart_removed && (
            <li>Legacy launch-at-login entry replaced</li>
          )}
        </ul>

        <p className="text-neutral-400 mt-3 text-[12px] leading-snug">
          Your old install at{" "}
          <code>~/Library/Application Support/com.claude-limits.ClaudeLimits/</code>{" "}
          is preserved as a fallback. After a few weeks of stable use you'll
          see a "tidy old data" option.
        </p>

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="px-3 py-1.5 rounded-md bg-teal-500/15 hover:bg-teal-500/25 text-teal-200 text-[12px] font-medium transition-colors"
          >
            Got it
          </button>
        </div>
      </div>
    </div>
  );
}
