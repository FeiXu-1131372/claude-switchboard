import { useEffect, useRef, useState, type ReactNode } from 'react';

interface Props {
  className?: string;
  children: ReactNode;
}

/**
 * A vertical scroll container that shows when there is more to see.
 *
 * The scrollbar alone did not: it is thin, sits on the window's edge, and on
 * Windows the system corner rounding and border can take most of it. So the
 * edge with hidden content also fades out (`data-more-above` /
 * `data-more-below`, styled in globals.css), which reads at a glance and
 * needs no extra chrome.
 */
export function ScrollArea({ className = '', children }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ above: false, below: false });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => {
      // One pixel of slack: zoomed webviews report fractional scroll offsets,
      // and the bottom is never reached exactly.
      const above = el.scrollTop > 1;
      const below = el.scrollTop + el.clientHeight < el.scrollHeight - 1;
      setEdges((prev) => (prev.above === above && prev.below === below ? prev : { above, below }));
    };
    update();
    el.addEventListener('scroll', update, { passive: true });
    // The viewport changes with the window, the content as cards load in.
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(update);
    observer?.observe(el);
    if (contentRef.current) observer?.observe(contentRef.current);
    return () => {
      el.removeEventListener('scroll', update);
      observer?.disconnect();
    };
  }, []);

  return (
    <div
      ref={ref}
      className={`scroll-area overflow-y-auto ${className}`}
      data-more-above={edges.above || undefined}
      data-more-below={edges.below || undefined}
    >
      <div ref={contentRef}>{children}</div>
    </div>
  );
}
