export const MODEL_VARIANT: Record<string, 'fable' | 'opus' | 'sonnet' | 'haiku' | 'default'> = {
  fable: 'fable',
  opus: 'opus',
  sonnet: 'sonnet',
  haiku: 'haiku',
};

export const MODEL_COLOR: Record<string, string> = {
  fable: 'var(--color-model-fable)',
  opus: 'var(--color-model-opus)',
  sonnet: 'var(--color-model-sonnet)',
  haiku: 'var(--color-model-haiku)',
};

export function modelKey(name: string): string {
  const lower = name.toLowerCase();
  if (lower.includes('fable') || lower.includes('mythos')) return 'fable';
  if (lower.includes('opus')) return 'opus';
  if (lower.includes('sonnet')) return 'sonnet';
  if (lower.includes('haiku')) return 'haiku';
  return 'default';
}

export function shortName(model: string): string {
  const stripped = model.replace(/\[1m\]$/i, '').replace(/^[^/]+\//, '');
  const m = stripped.match(/(fable|mythos|opus|sonnet|haiku)-(\d+(?:[.-]\d+)?)/i);
  if (m) {
    return `${m[1].toLowerCase()} ${m[2].replace(/\./g, '-')}`;
  }
  return stripped.replace(/-\d{8}$/, '');
}

/** Badge text for a model. Anthropic families collapse to their tier name
 * (fable/opus/sonnet/haiku); third-party / relay models keep a cleaned
 * version of their real name so the badge says "glm-5.3" or "k3" instead of
 * a generic "default" — the family color variant is still driven by
 * `modelKey` (which stays "default", i.e. neutral, for non-Anthropic). */
export function modelLabel(name: string): string {
  const stripped = name.replace(/\[1m\]$/i, '').replace(/^[^/]+\//, '');
  const lower = stripped.toLowerCase();
  if (lower.includes('fable') || lower.includes('mythos')) return 'fable';
  if (lower.includes('opus')) return 'opus';
  if (lower.includes('sonnet')) return 'sonnet';
  if (lower.includes('haiku')) return 'haiku';
  return stripped
    .replace(/-(highspeed|streaming)$/i, '')
    .replace(/-\d{8}$/, '')
    .replace(/-for-coding$/i, '');
}
