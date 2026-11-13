import { type InputHTMLAttributes, forwardRef, useId } from 'react';

interface ToggleProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> {
  label: string;
  description?: string;
}

export const Toggle = forwardRef<HTMLInputElement, ToggleProps>(
  ({ label, description, className = '', ...props }, ref) => {
    const id = useId();

    return (
      <label
        htmlFor={id}
        className={[
          'flex items-center justify-between gap-[var(--space-md)] cursor-pointer',
          'has-[:disabled]:opacity-40 has-[:disabled]:cursor-not-allowed',
          'py-[var(--space-xs)]',
          className,
        ].join(' ')}
      >
        <div className="flex flex-col gap-[2px]">
          <span className="text-[length:var(--text-body)] font-[var(--weight-medium)] text-[color:var(--color-text)]">
            {label}
          </span>
          {description && (
            <span className="text-[length:var(--text-micro)] text-[color:var(--color-text-muted)]">
              {description}
            </span>
          )}
        </div>
        <div className="relative shrink-0">
          <input
            ref={ref}
            id={id}
            type="checkbox"
            role="switch"
            className="peer sr-only"
            {...props}
          />
          <div
            className={[
              'w-[36px] h-[20px] rounded-[var(--radius-pill)]',
              'bg-[var(--color-track)] border border-[var(--color-border)]',
              'transition-[background,border-color] duration-[var(--duration-fast)] ease-[var(--ease-out)]',
              'peer-checked:bg-[var(--color-accent)] peer-checked:border-transparent',
              'peer-focus-visible:outline-2 peer-focus-visible:outline-[var(--color-border-focus)] peer-focus-visible:outline-offset-2',
            ].join(' ')}
          />
          <div
            className={[
              'absolute top-[3px] left-[3px]',
              'w-[14px] h-[14px] rounded-full',
              'bg-[var(--color-text-muted)]',
              'transition-[transform,background] duration-[var(--duration-fast)] ease-[var(--ease-spring)]',
              'peer-checked:translate-x-[16px] peer-checked:bg-white',
            ].join(' ')}
          />
        </div>
      </label>
    );
  },
);

Toggle.displayName = 'Toggle';
