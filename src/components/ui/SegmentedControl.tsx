/** A small set of mutually exclusive choices, shown side by side. */
export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div
      role="group"
      aria-label={label}
      className="flex gap-[var(--space-2xs)] bg-[var(--color-track)] rounded-[var(--radius-sm)] p-[2px] w-fit"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
          className={[
            'px-[var(--space-sm)] py-[var(--space-2xs)]',
            'text-[length:var(--text-label)] font-[var(--weight-medium)]',
            'rounded-[var(--radius-sm)]',
            'transition-[background,color] duration-[var(--duration-fast)]',
            value === option.value
              ? 'bg-[var(--color-bg-card)] text-[color:var(--color-text)]'
              : 'text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text-secondary)]',
          ].join(' ')}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
