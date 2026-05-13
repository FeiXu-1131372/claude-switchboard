import { ModalShell } from './ModalShell';
import { SettingsPanel } from '../../settings/SettingsPanel';

interface Props {
  onDismiss: () => void;
}

export function SettingsModal({ onDismiss }: Props) {
  return (
    <ModalShell id="settings-modal" onDismiss={onDismiss} size="lg" title="Settings">
      <div className="px-[var(--space-md)] py-[var(--space-md)] max-h-[480px] overflow-y-auto">
        <SettingsPanel />
      </div>
    </ModalShell>
  );
}
