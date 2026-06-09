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
