// Small pure helpers shared by both directions of the
// Rust⇄FortuneSheet cell-style conversion.
//
// Nothing in this file touches FortuneSheet or Rust types directly —
// only primitive `string`s and `number`s. They are split out so the
// obscure normalization logic (e.g. why `centre` and `center` are
// equivalent) lives in one place with its own tests.

/**
 * Normalise a colour string from Rust/OOXML to the CSS hex colour
 * FortuneSheet expects. OOXML can produce any of:
 *
 *   - #RRGGBB    (6-char with hash — from Rust after our fix)
 *   - #AARRGGBB  (8-char with hash — rare, from OOXML direct)
 *   - RRGGBB     (6-char without hash — old Rust or direct OOXML)
 *   - AARRGGBB   (8-char without hash — direct OOXML)
 *
 * FortuneSheet expects `#RRGGBB` (6-char with hash). Any input that
 * doesn't look like a hex ARGB/RGB triplet is returned as-is so
 * named colours / `rgb(…)` etc. pass through.
 */
export function normaliseColor(color: string | undefined): string | undefined {
  if (!color) return undefined;
  // Already well-formed with hash
  if (/^#[0-9a-fA-F]{6}$/.test(color)) return color.toLowerCase();
  // 8-char ARGB with hash → strip alpha
  if (/^#[0-9a-fA-F]{8}$/.test(color)) return ('#' + color.slice(3)).toLowerCase();
  // 6-char without hash
  if (/^[0-9a-fA-F]{6}$/.test(color)) return '#' + color.toLowerCase();
  // 8-char ARGB without hash → strip alpha
  if (/^[0-9a-fA-F]{8}$/.test(color)) return ('#' + color.slice(2)).toLowerCase();
  // Named colours, rgb(), etc. — return as-is
  return color;
}

/**
 * Convert Rust horizontal alignment string to the FortuneSheet `ht`
 * value: 0 = center, 1 = left, 2 = right.
 * Accepts both US `center` and UK `centre` spellings.
 */
export function alignH(h: string | undefined): number | undefined {
  if (!h) return undefined;
  switch (h) {
    case 'center':
    case 'centre':
      return 0;
    case 'left':
      return 1;
    case 'right':
      return 2;
    default:
      return undefined;
  }
}

/**
 * Convert Rust vertical alignment string to the FortuneSheet `vt`
 * value: 0 = middle, 1 = top, 2 = bottom. `center` here is accepted
 * as a synonym of `middle` (matches the Rust backend).
 */
export function alignV(v: string | undefined): number | undefined {
  if (!v) return undefined;
  switch (v) {
    case 'center':
      return 0;
    case 'top':
      return 1;
    case 'bottom':
      return 2;
    default:
      return undefined;
  }
}
