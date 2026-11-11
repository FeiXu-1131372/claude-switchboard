import { type ButtonHTMLAttributes, forwardRef } from 'react';

type ButtonVariant = 'primary' | 'ghost' | 'destructive';
type ButtonSize = 'sm' | 'md';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

const variantClasses: Record<ButtonVariant, string> = {
  primary:
    'bg-[var(--color-accent)] text-[color:var(--color-on-accent)] hover:brightness-110 active:brightness-95',
  ghost:
    'bg-transparent text-[color:var(--color-text-secondary)] hover:bg-[var(--color-bg-card)] hover:text-[color:var(--color-text)]',
  destructive:
    'bg-[var(--color-danger)] text-white hover:brightness-110 active:brightness-95',
};

const sizeClasses: Record<ButtonSize, string> = {
  sm: 'px-[var(--space-sm)] py-[var(--space-2xs)] text-[length:var(--text-label)] rounded-[var(--radius-sm)]',
  md: 'px-[var(--space-md)] py-[var(--space-xs)] text-[length:var(--text-body)] rounded-[var(--radius-sm)]',
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'ghost', size = 'md', className = '', ...props }, ref) => (
    <button
      ref={ref}
      className={[
        'inline-flex items-center justify-center gap-[var(--space-xs)]',
        'font-[var(--weight-medium)] select-none',
        'transition-[background,color,opacity] duration-[var(--duration-fast)] ease-[var(--ease-out)]',
        'focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-1',
        'disabled:opacity-40 disabled:pointer-events-none',
        variantClasses[variant],
        sizeClasses[size],
        className,
      ].join(' ')}
      {...props}
    />
  ),
);

Button.displayName = 'Button';
