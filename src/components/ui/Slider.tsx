import { type InputHTMLAttributes, forwardRef, useId, useState } from 'react';

interface SliderProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> {
  label: string;
  /** Display format for the value, e.g. (v) => `${v}m` */
  formatValue?: (value: number) => string;
  min?: number;
  max?: number;
  step?: number;
}

export const Slider = forwardRef<HTMLInputElement, SliderProps>(
  ({
    label,
    formatValue = (v) => String(v),
    min = 0,
    max = 100,
    step = 1,
    defaultValue,
    value: controlledValue,
    onChange,
    className = '',
    ...props
  }, ref) => {
    const id = useId();
    const controlled = controlledValue !== undefined;
    const [internalValue, setInternalValue] = useState(
      Number(defaultValue ?? min),
    );
    const value = controlled ? Number(controlledValue) : internalValue;

    const pct = ((value - min) / (max - min)) * 100;

    return (
      // Dimmed when off, as Button is: a disabled slider in full accent read
      // as live (rotation's thresholds while rotation was off).
      <div className={['flex flex-col gap-[var(--space-xs)] has-[:disabled]:opacity-40', className].join(' ')}>
        <div className="flex items-center justify-between">
          <label
            htmlFor={id}
            className="text-[length:var(--text-label)] font-[var(--weight-medium)] text-[color:var(--color-text-secondary)]"
          >
            {label}
          </label>
          <span className="mono text-[length:var(--text-label)] font-[var(--weight-semibold)] text-[color:var(--color-accent-text)]">
            {formatValue(value)}
          </span>
        </div>
        <div className="relative h-[20px] flex items-center">
          <input
            ref={ref}
            id={id}
            type="range"
            min={min}
            max={max}
            step={step}
            value={value}
            onChange={(e) => {
              if (!controlled) setInternalValue(Number(e.target.value));
              onChange?.(e);
            }}
            className={[
              'w-full h-[5px] appearance-none rounded-[var(--radius-pill)]',
              'bg-[var(--color-track)] outline-none',
              'cursor-pointer disabled:cursor-not-allowed',
              'focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] focus-visible:ring-offset-1',
              '[&::-webkit-slider-thumb]:appearance-none',
              '[&::-webkit-slider-thumb]:w-[16px] [&::-webkit-slider-thumb]:h-[16px]',
              '[&::-webkit-slider-thumb]:rounded-full',
              '[&::-webkit-slider-thumb]:bg-[var(--color-accent)]',
              '[&::-webkit-slider-thumb]:border-2 [&::-webkit-slider-thumb]:border-[var(--color-bg-base)]',
              '[&::-webkit-slider-thumb]:cursor-pointer',
              '[&::-webkit-slider-thumb]:transition-transform [&::-webkit-slider-thumb]:duration-[var(--duration-fast)]',
              '[&::-webkit-slider-thumb]:ease-[var(--ease-spring)]',
              '[&::-webkit-slider-thumb]:hover:scale-110',
              '[&::-webkit-slider-thumb]:active:scale-95',
            ].join(' ')}
            style={{
              background: `linear-gradient(to right, var(--color-accent) 0%, var(--color-accent) ${pct}%, var(--color-track) ${pct}%, var(--color-track) 100%)`,
            }}
            {...props}
          />
        </div>
      </div>
    );
  },
);

Slider.displayName = 'Slider';
