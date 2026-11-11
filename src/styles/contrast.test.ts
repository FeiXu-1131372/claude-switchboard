/**
 * Text colours must stay readable on every surface they are used on. This
 * reads the real token file, so a token edit that drops a pair below WCAG AA
 * (4.5:1 for small text) fails here rather than in someone's eyes.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { PALETTES } from '../lib/theme';

type Rgb = [number, number, number];
type Tokens = Map<string, string>;

const css = readFileSync(path.resolve(__dirname, 'tokens.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');

function declarations(block: string): Tokens {
  const out: Tokens = new Map();
  for (const m of block.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out.set(m[1], m[2].trim());
  return out;
}

function blockAfter(marker: string): string {
  const start = css.indexOf(marker);
  if (start < 0) throw new Error(`tokens.css has no "${marker}" block`);
  const open = css.indexOf('{', start);
  return css.slice(open + 1, css.indexOf('}', open));
}

const LIGHT = declarations(blockAfter('@theme'));
const DARK = declarations(blockAfter('body[data-theme="dark"]'));

/** The original colour theme is the token file's base; each other one is a
 * pair of blocks that restate only what they change. */
const ORIGINAL = PALETTES[0].id;
const paletteLight = (id: string) => declarations(blockAfter(`body[data-palette="${id}"] {`));
const paletteDark = (id: string) => declarations(blockAfter(`body[data-palette="${id}"][data-theme="dark"] {`));

/** Every colour theme in both modes, each built the way the cascade builds
 * it: later blocks win, and anything a block leaves out comes from before. */
const THEMES: Record<string, Tokens> = {
  light: LIGHT,
  dark: new Map([...LIGHT, ...DARK]),
};
for (const { id } of PALETTES) {
  if (id === ORIGINAL) continue;
  THEMES[`${id} light`] = new Map([...LIGHT, ...paletteLight(id)]);
  THEMES[`${id} dark`] = new Map([...LIGHT, ...DARK, ...paletteLight(id), ...paletteDark(id)]);
}

function resolve(tokens: Tokens, name: string): string {
  let value = tokens.get(name);
  for (let hops = 0; value && hops < 8; hops++) {
    const ref = value.match(/^var\((--[\w-]+)\)$/);
    if (!ref) return value;
    value = tokens.get(ref[1]);
  }
  throw new Error(`token ${name} is not defined`);
}

/** `oklch(L% C h / a)` → gamma-encoded sRGB in 0..1, plus alpha. */
function parseOklch(value: string): { rgb: Rgb; alpha: number } {
  const m = value.match(/^oklch\(\s*([\d.]+)%\s+([\d.]+)\s+([\d.]+)(?:\s*\/\s*([\d.]+))?\s*\)$/);
  if (!m) throw new Error(`not an oklch colour: ${value}`);
  const L = Number(m[1]) / 100;
  const C = Number(m[2]);
  const h = (Number(m[3]) * Math.PI) / 180;
  const a = C * Math.cos(h);
  const b = C * Math.sin(h);

  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const mm = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const linear: Rgb = [
    4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * s,
  ];
  const encode = (c: number) => {
    const v = Math.min(1, Math.max(0, c));
    return v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055;
  };
  return { rgb: linear.map(encode) as Rgb, alpha: m[4] === undefined ? 1 : Number(m[4]) };
}

function luminance(rgb: Rgb): number {
  const [r, g, b] = rgb.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast of text token `fg` on surface token `bg`. Translucent text
 * is composited over the surface first, the way the browser paints it. */
function ratio(theme: string, fg: string, bg: string): number {
  const tokens = THEMES[theme];
  const surface = parseOklch(resolve(tokens, bg)).rgb;
  const text = parseOklch(resolve(tokens, fg));
  const painted = text.rgb.map((c, i) => c * text.alpha + surface[i] * (1 - text.alpha)) as Rgb;
  const [hi, lo] = [luminance(painted), luminance(surface)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const TEXT = ['--color-text-muted', '--color-warn-text', '--color-accent-text', '--color-text-secondary'];
const SURFACES = ['--color-bg-base', '--color-bg-surface', '--color-bg-card'];

describe('the contrast maths', () => {
  it('gives 21:1 for black on white and 1:1 for a colour on itself', () => {
    const white: Rgb = [1, 1, 1];
    const black: Rgb = [0, 0, 0];
    expect((luminance(white) + 0.05) / (luminance(black) + 0.05)).toBeCloseTo(21, 5);
    expect(ratio('light', '--color-bg-base', '--color-bg-base')).toBeCloseTo(1, 5);
  });

  it('converts oklch to sRGB', () => {
    // oklch(62.8% 0.2577 29.23) is sRGB red.
    const { rgb } = parseOklch('oklch(62.8% 0.2577 29.23)');
    expect(rgb[0]).toBeCloseTo(1, 2);
    expect(rgb[1]).toBeCloseTo(0, 1);
    expect(rgb[2]).toBeCloseTo(0, 1);
  });
});

describe('text contrast', () => {
  for (const theme of Object.keys(THEMES))
    for (const fg of TEXT)
      for (const bg of SURFACES)
        it(`${theme}: ${fg} on ${bg} is at least 4.5:1`, () => {
          expect(ratio(theme, fg, bg)).toBeGreaterThanOrEqual(4.5);
        });
});

describe('text on the accent fill', () => {
  // Buttons, the Active tag and the switch notice put text on the accent
  // itself. The accent is dark in light mode and light in dark mode, so the
  // text on it has to flip too: white on a light accent is unreadable.
  for (const theme of Object.keys(THEMES))
    it(`${theme}: --color-on-accent on --color-accent is at least 4.5:1`, () => {
      expect(ratio(theme, '--color-on-accent', '--color-accent')).toBeGreaterThanOrEqual(4.5);
    });
});

describe('colour themes', () => {
  const others = PALETTES.filter((p) => p.id !== ORIGINAL);

  it('there is a light and a dark version of each', () => {
    expect(Object.keys(THEMES)).toHaveLength(PALETTES.length * 2);
  });

  it.each(others)('$id sets its own surfaces, text and accent in both modes', ({ id }) => {
    for (const block of [paletteLight(id), paletteDark(id)]) {
      for (const token of ['--color-bg-base', '--color-bg-card', '--color-text', '--color-text-muted', '--color-accent', '--color-accent-text']) {
        expect(block.has(token), `${id} is missing ${token}`).toBe(true);
      }
    }
  });

  // A theme's light block comes after the base dark block and has the same
  // weight, so in dark mode it would win. The theme's dark block has to take
  // back every token its light block set, or a light value leaks into dark.
  it.each(others)('$id dark restates everything $id light sets', ({ id }) => {
    const dark = paletteDark(id);
    const leaked = [...paletteLight(id).keys()].filter((token) => !dark.has(token));
    expect(leaked).toEqual([]);
  });

  it.each(PALETTES)('the $id swatch is that theme\u2019s accent', ({ id }) => {
    const root = declarations(blockAfter(':root'));
    const accent = id === ORIGINAL ? LIGHT.get('--color-accent') : paletteLight(id).get('--color-accent');
    expect(root.get(`--swatch-${id}`)).toBe(accent);
  });

  it('no two themes share an accent', () => {
    const accents = PALETTES.map(({ id }) => resolve(THEMES[id === ORIGINAL ? 'light' : `${id} light`], '--color-accent'));
    expect(new Set(accents).size).toBe(PALETTES.length);
  });
});
