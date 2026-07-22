/**
 * Shared constants for the svgEditor module.
 *
 * NOTE: this file is currently a placeholder so that the other modules in
 * this directory (`parseSvg.ts`, `serializeSvg.ts`, `useSelection.ts`) can
 * type-check and resolve imports. The real definitions — including the
 * authoritative values for `EDIT_HANDLE_ATTR`, the per-tag `KIND_BY_TAG`
 * metadata table, the wrappability rule (`NON_WRAPPABLE_TAGS`), and the
 * `nextHandleId` counter — will be filled in as the rest of the editor
 * lands. Until then the values below are *minimal stand-ins*, not the
 * final API surface.
 */

export const EDIT_HANDLE_ATTR = "data-svg-edit-handle";

export interface TagKind {
  /** Whether an element of this tag may become a selectable wrapper. */
  selectable: boolean;
}

export const KIND_BY_TAG: Record<string, TagKind | undefined> = {
  rect: { selectable: true },
  circle: { selectable: true },
  ellipse: { selectable: true },
  line: { selectable: true },
  polyline: { selectable: true },
  polygon: { selectable: true },
  path: { selectable: true },
  text: { selectable: true },
  image: { selectable: true },
  g: { selectable: true },
};

export const NON_WRAPPABLE_TAGS = new Set<string>([
  "defs",
  "title",
  "desc",
  "metadata",
  "style",
  "script",
  "clipPath",
  "filter",
  "mask",
  "linearGradient",
  "radialGradient",
  "pattern",
  "marker",
  "symbol",
]);

let handleCounter = 0;
export function nextHandleId(): string {
  handleCounter += 1;
  return `svg-edit-${handleCounter}`;
}