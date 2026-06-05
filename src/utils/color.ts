/** Color manipulation utilities. */

const PERCENT_TO_AMOUNT = 2.55;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function parseHexChannel(hex: string): number {
  const cleaned = hex.replace('#', '');
  const normalized = cleaned.length === 3
    ? cleaned.split('').map((c) => c + c).join('')
    : cleaned;
  const num = parseInt(normalized, 16);
  return isNaN(num) ? -1 : num;
}

/**
 * Adjusts the brightness of a hex color by a given percentage.
 * Positive percent brightens, negative darkens.
 *
 * @param hex - A 3-char (#RGB) or 6-char (#RRGGBB) hex color string.
 * @param percent - Percentage to shift (-100 to 100 range is meaningful; clamped beyond).
 * @returns The adjusted hex color string, or the original if the input is invalid.
 */
export function adjustColor(hex: string, percent: number): string {
  const rgb = parseHexChannel(hex);
  if (rgb < 0) return hex;

  const amount = Math.round(PERCENT_TO_AMOUNT * percent);
  const red = clamp((rgb >> 16) + amount, 0, 255);
  const green = clamp(((rgb >> 8) & 0x00ff) + amount, 0, 255);
  const blue = clamp((rgb & 0x0000ff) + amount, 0, 255);

  return `#${(0x1000000 + red * 0x10000 + green * 0x100 + blue).toString(16).slice(1)}`;
}

// ─── Tests ───────────────────────────────────────────────────────────────────

const _TESTS: Array<[string, number, string]> = [
  ['#ffffff', 0, '#ffffff'],
  ['#ffffff', 20, '#ffffff'],
  ['#000000', 20, '#333333'],
  ['#ff0000', -20, '#cc0000'],
  ['#00ff00', 20, '#33ff33'],
  ['#0000ff', -20, '#0000cc'],
  ['#fff', 0, '#ffffff'],
  ['#123', 10, '#1a1a33'],
  ['#aabbcc', 10, '#b8c4d2'],
  ['#aabbcc', -10, '#99a7b3'],
  ['invalid', 10, 'invalid'],
  ['#XYZ', 0, '#XYZ'],
];

// Run tests in development
if (import.meta.env.DEV) {
  let passed = 0;
  for (const [input, percent, expected] of _TESTS) {
    const actual = adjustColor(input, percent);
    if (actual === expected) {
      passed++;
    } else {
      console.error(`adjustColor(${JSON.stringify(input)}, ${percent}) = ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`);
    }
  }
  console.debug(`[color] ${passed}/${_TESTS.length} tests passed`);
}
