import { ModalShell } from './ModalShell';
import { SettingsPanel } from '../../settings/SettingsPanel';

interface Props {
  onDismiss: () => void;
}

export function SettingsModal({ onDismiss }: Props) {
  return (
    <ModalShell id="settings-modal" onDismiss={onDismiss} size="lg" title="Settings">
      {/* The panel scrolls itself and fills the modal, which stops at the window. */}
      <SettingsPanel bodyClassName="p-[var(--space-md)]" />
    </ModalShell>
  );
}
