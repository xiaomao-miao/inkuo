// Numeric helpers used across the Word toolbar.
//
// Pure, dependency-free functions for clamping / snapping values into the
// ranges accepted by various editor commands. Keeping them here means the
// constants (e.g. `FONT_SIZE_MAX_PT`) are named once and unit-tested rather
// than scattered as magic numbers throughout the toolbar files.

/** Inclusive upper bound for a font size in points. Word's smallest "huge" size is 96pt; we round up to 400 for power users. */
export const FONT_SIZE_MAX_PT = 400;
/** Inclusive lower bound for a font size in points. */
export const FONT_SIZE_MIN_PT = 1;

/** Inclusive upper bound for the zoom dropdown (in percent). */
export const ZOOM_MAX_PCT = 500;
/** Inclusive lower bound for the zoom dropdown (in percent). */
export const ZOOM_MIN_PCT = 1;

/**
 * Clamp a font size value into the legal `[1, 400]` pt range. Non-finite
 * inputs collapse to the supplied fallback.
 */
export function clampFontSizePt(value: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.max(FONT_SIZE_MIN_PT, Math.min(FONT_SIZE_MAX_PT, Math.round(value)));
}

/**
 * Validate a font size entered as free text. Returns the clamped value when
 * valid, or `null` if the input is unparseable / out of range.
 */
export function parseFontSizePt(raw: string): number | null {
  const n = Number(raw);
  if (!Number.isFinite(n) || n < FONT_SIZE_MIN_PT || n > FONT_SIZE_MAX_PT) return null;
  return n;
}

/**
 * Convert a percentage (e.g. `75`) into a zoom factor (e.g. `0.75`). Validates
 * the percentage is in range; returns `null` on invalid input.
 */
export function parseZoomFactorFromPct(raw: string): number | null {
  const n = Number(raw);
  if (!Number.isFinite(n) || n < ZOOM_MIN_PCT || n > ZOOM_MAX_PCT) return null;
  return n / 100;
}

/** Half-points: the underlying PM unit for font size. Multiply pt × 2 to get half-points. */
export function ptToHalfPoints(pt: number): number {
  return Math.round(pt * 2);
}

/**
 * Clamp a font-size step (e.g. +1 or -1 from the spinner) into a safe next
 * value. Convenience wrapper around `clampFontSizePt` for the stepper case.
 */
export function stepFontSizePt(currentPt: number, delta: number, fallback: number): number {
  return clampFontSizePt(currentPt + delta, fallback);
}