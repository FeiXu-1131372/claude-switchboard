import { type HTMLAttributes, forwardRef, type ReactNode } from 'react';

type BannerVariant = 'info' | 'warning' | 'error' | 'stale';

interface BannerProps extends HTMLAttributes<HTMLDivElement> {
  variant?: BannerVariant;
  icon?: ReactNode;
  children: ReactNode;
  onDismiss?: () => void;
}

const variantClasses: Record<BannerVariant, string> = {
  info: 'bg-[var(--color-accent-dim)] border-[var(--color-accent)]/20 text-[color:var(--color-accent-text)]',
  warning: 'bg-[var(--color-warn-dim)] border-[var(--color-warn)]/20 text-[color:var(--color-warn-text)]',
  error: 'bg-[var(--color-danger-dim)] border-[var(--color-danger)]/20 text-[color:var(--color-danger)]',
  stale: 'bg-[var(--color-track)] border-[var(--color-border)] text-[color:var(--color-text-secondary)]',
};

export const Banner = forwardRef<HTMLDivElement, BannerProps>(
  ({ variant = 'info', icon, children, onDismiss, className = '', ...props }, ref) => (
    <div
      ref={ref}
      role="alert"
      className={[
        'flex items-center gap-[var(--space-sm)]',
        'px-[var(--space-md)] py-[var(--space-sm)]',
        'rounded-[var(--radius-sm)] border',
        'text-[length:var(--text-label)] font-[var(--weight-medium)]',
        variantClasses[variant],
        className,
      ].join(' ')}
      {...props}
    >
      {icon && <span className="shrink-0">{icon}</span>}
      <span className="flex-1">{children}</span>
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          className="shrink-0 opacity-60 hover:opacity-100 transition-opacity"
          aria-label="Dismiss"
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M18 6 6 18" /><path d="m6 6 12 12" />
          </svg>
        </button>
      )}
    </div>
  ),
);

Banner.displayName = 'Banner';
