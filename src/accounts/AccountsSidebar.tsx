import { motion } from 'framer-motion';
import { useState, type Ref } from 'react';
import { useAppStore } from '../lib/store';
import { IconButton } from '../components/ui/IconButton';
import { AccountRow } from './AccountRow';
import { AddAccountChooser } from './AddAccountChooser';
import { SwapConfirmCard } from './SwapConfirmCard';
import { ModalShell } from '../components/modals/ModalShell';
import { IconRefresh, IconSidebarHide } from '../lib/icons';
import { handleDragStart } from '../lib/window-chrome';
import { useAccountManagement } from './useAccountManagement';

export function AccountsSidebar({
  onHide,
  hideButtonRef,
}: {
  /** Given by a window that can show its content without this column. */
  onHide?: () => void;
  hideButtonRef?: Ref<HTMLButtonElement>;
} = {}) {
  const [importBusy, setImportBusy] = useState(false);
  const thresholds = useAppStore(
    (s) => (s.settings?.thresholds ?? [75, 90]) as [number, number],
  );

  const {
    accounts,
    orgGroups,
    currentActive,
    pending,
    swappingSlot,
    confirmError,
    refreshing,
    reauthSlot,
    chooserOpen,
    tokenAccount,
    requestSwap,
    confirmSwap,
    cancelSwap,
    handleReauth,
    handleRemove,
    handleRefreshAll,
    openChooser,
    closeChooser,
  } = useAccountManagement();

  return (
    <aside
      className="
        flex flex-col h-full
        w-[var(--sidebar-width)] shrink-0
        border-r border-[var(--color-rule)]
        bg-[var(--color-bg-surface)]
      "
    >
      {/* Level with the window's header row, and like it a place to take hold
          of the window: with the tabs in that row, this is most of the room
          there is to drag by. */}
      <div
        onPointerDown={handleDragStart}
        className="
          flex h-[var(--shell-header-height)] shrink-0 items-center justify-between
          border-b border-[var(--color-rule)]
          pl-[var(--popover-pad)] pr-[var(--space-sm)]
          cursor-default select-none
        "
      >
        <span className="text-[length:var(--text-label)] uppercase tracking-[var(--tracking-label)] text-[color:var(--color-text-secondary)] font-[var(--weight-semibold)]">
          Accounts
        </span>
        <div className="flex items-center gap-[var(--space-2xs)]">
          <IconButton label="Refresh all" onClick={handleRefreshAll}>
            <motion.span
              animate={refreshing ? { rotate: 360 } : { rotate: 0 }}
              transition={
                refreshing
                  ? { duration: 0.7, ease: 'linear', repeat: Infinity }
                  : { duration: 0.2 }
              }
              style={{ display: 'inline-flex' }}
            >
              <IconRefresh size={13} />
            </motion.span>
          </IconButton>
          {onHide && (
            <IconButton ref={hideButtonRef} label="Hide accounts" onClick={onHide}>
              <IconSidebarHide size={14} />
            </IconButton>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        {accounts.length === 0 && (
          <div className="px-[var(--popover-pad)] py-[var(--space-md)] text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
            No accounts managed yet.
          </div>
        )}
        {accounts.map((a) => {
          const groupHead = a.org_uuid ? orgGroups.get(a.org_uuid) : undefined;
          const shareHint = groupHead && groupHead.slot !== a.slot ? groupHead.email : null;
          return (
            <AccountRow
              key={a.slot}
              entry={a}
              thresholds={thresholds}
              shareHint={shareHint}
              onSwap={() => requestSwap(a)}
              swapBusy={swappingSlot !== null}
              swapping={swappingSlot === a.slot}
              onReauth={() => handleReauth(a)}
              reauthBusy={reauthSlot === a.slot}
              onRemove={() => handleRemove(a)}
            />
          );
        })}
      </div>

      <div className="shrink-0 px-[var(--popover-pad)] py-[var(--space-md)] border-t border-[var(--color-rule)]">
        <button
          type="button"
          onClick={openChooser}
          className="text-[length:var(--text-label)] text-[color:var(--color-accent-text)] hover:underline"
        >
          + Add account
        </button>
      </div>

      {chooserOpen && (
        <ModalShell id="add-account-chooser" onDismiss={closeChooser} title={tokenAccount ? 'Replace token' : 'Add account'} dismissable={!importBusy}>
          <AddAccountChooser presentation="modal" onClose={closeChooser} tokenAccount={tokenAccount} onBusyChange={setImportBusy} />
        </ModalShell>
      )}

      {pending && (
        <ModalShell id="swap-confirm" onDismiss={cancelSwap} title="Confirm switch">
          <SwapConfirmCard
            presentation="modal"
            current={currentActive}
            target={pending.target}
            running={pending.running}
            busy={swappingSlot !== null}
            errorMessage={confirmError}
            onConfirm={confirmSwap}
            onCancel={cancelSwap}
          />
        </ModalShell>
      )}
    </aside>
  );
}
