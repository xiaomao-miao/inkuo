import { EDIT_HANDLE_ATTR, NON_WRAPPABLE_TAGS, nextHandleId, KIND_BY_TAG } from "./types";

export interface ParsedSvg {
  /**
   * The original on-disk SVG text (decoded from base64). Used to
   * preserve any leading XML declaration / doctype when re-serializing.
   */
  originalText: string;
  /** The XML namespace URI detected on the root, e.g. `http://www.w3.org/2000/svg`. */
  namespace: string;
  /**
   * The serialized body to feed `dangerouslySetInnerHTML`. We serialize
   * the prepared DOM back out (rather than handing in `originalText`)
   * so the wrapper `<g data-svg-edit-handle>` groups we add are
   * present in the live tree.
   */
  innerHtml: string;
  /**
   * Width / height of the SVG in user units (the `viewBox` / width /
   * height values, normalized to numbers). `null` if the file has none
   * of those — in that case the editor renders at a sensible default
   * (300×150) and lets the user resize.
   */
  intrinsic: { w: number; h: number } | null;
}

const SVG_NS = "http://www.w3.org/2000/svg";

/**
 * Decode a base64-encoded SVG blob (as returned by `read_file_for_viewer`)
 * and prepare it for interactive editing. Steps:
 *
 *   1. base64-decode → string
 *   2. `DOMParser` → `SVGSVGElement` (sanity-checked)
 *   3. Backfill a `viewBox` if missing (so coordinate math is stable)
 *   4. Wrap every selectable direct child in `<g data-svg-edit-handle>`
 *   5. Recurse one level into existing `<g>` so users can edit grouped
 *      shapes too (we deliberately stop there to avoid creating deeply
 *      nested wrappers that would bloat the saved file).
 *   6. Serialize the prepared DOM back out
 *
 * Returns `null` if the payload can't be parsed. Callers should fall
 * back to the `<img>` rendering path in that case.
 */
export function parseAndPrepareSvg(dataBase64: string): ParsedSvg | null {
  let rawText: string;
  try {
    rawText = atob(dataBase64);
  } catch {
    return null;
  }

  const parser = new DOMParser();
  const doc = parser.parseFromString(rawText, "image/svg+xml");
  // DOMParser reports malformed XML via a `<parsererror>` element on
  // the document rather than throwing. Walk up from it to see if it
  // appeared.
  if (doc.getElementsByTagName("parsererror").length > 0) {
    return null;
  }

  const root = doc.documentElement;
  if (!root || root.namespaceURI !== SVG_NS || root.localName !== "svg") {
    return null;
  }

  // Promote the documentElement to a proper SVGSVGElement so getBBox()
  // / createSVGPoint() / getScreenCTM() are available. The DOMParser
  // result is already the right kind in browsers, but the cast makes
  // TS happy and guards against polyfilled environments.
  const svg = root as unknown as SVGSVGElement;
  const intrinsic = readDimensions(svg);
  ensureViewBox(svg, intrinsic);

  wrapSelectableChildren(svg);

  // Some SVG files ship with a `<?xml ...?>` declaration; we want to
  // keep it on save, so extract it before serialization.
  const xmlDeclMatch = rawText.match(/^<\?xml[^>]*\?>\s*/);
  const doctypeMatch = rawText.match(/^<!DOCTYPE[^>]*>\s*/);

  const serializer = new XMLSerializer();
  let innerHtml = serializer.serializeToString(svg);
  if (xmlDeclMatch) innerHtml = xmlDeclMatch[0] + innerHtml;
  else if (doctypeMatch) innerHtml = doctypeMatch[0] + innerHtml;

  return {
    originalText: rawText,
    namespace: SVG_NS,
    innerHtml,
    intrinsic,
  };
}

/**
 * Walk the (now-prepared) SVG root and stamp a unique id on each
 * wrapper `<g>` if missing. We do this so the React side has a stable
 * way to address handles across renders, but it isn't required for
 * correctness — the DOM refs are.
 */
export function ensureHandleIds(svgRoot: SVGSVGElement): void {
  const wrappers = svgRoot.querySelectorAll(`g[${EDIT_HANDLE_ATTR}]`);
  wrappers.forEach((node) => {
    const el = node as SVGGElement;
    if (!el.getAttribute("id")) {
      el.setAttribute("id", nextHandleId());
    }
  });
}

/** Re-extract the wrapper `<g>` for a given selectable element. */
export function findWrapperFor(target: SVGElement): SVGGElement | null {
  // The wrapper is the direct parent of the original element. We
  // recorded it via `data-svg-edit-handle` during preparation, so the
  // closest ancestor with that attribute is exactly what we want.
  let parent = target.parentElement;
  while (parent && parent.getAttribute(EDIT_HANDLE_ATTR) === null) {
    parent = parent.parentElement;
  }
  return parent ? (parent as unknown as SVGGElement) : null;
}

/** Inverse of `findWrapperFor` — return the original element a wrapper holds. */
export function findOriginalFor(wrapper: SVGGElement): SVGElement | null {
  for (const child of Array.from(wrapper.children)) {
    if ((child as SVGElement).getAttribute?.(EDIT_HANDLE_ATTR) === null) {
      return child as SVGElement;
    }
  }
  return null;
}

function wrapSelectableChildren(svg: SVGSVGElement): void {
  // Snapshot the children list because we'll be mutating it.
  const directChildren = Array.from(svg.children);
  for (const child of directChildren) {
    if (!isWrappable(child)) continue;
    wrapElement(child as SVGElement);
    // If the original element was a `<g>`, recurse one level so its
    // inner shapes also become individually selectable. We don't go
    // deeper to avoid deeply-nested wrappers.
    if (child.localName === "g") {
      const innerChildren = Array.from((child as SVGGElement).children);
      for (const inner of innerChildren) {
        if (isWrappable(inner) && inner.getAttribute(EDIT_HANDLE_ATTR) === null) {
          wrapElement(inner as SVGElement);
        }
      }
    }
  }
}

function wrapElement(el: SVGElement): void {
  const parent = el.parentNode;
  if (!parent) return;
  const g = parent.ownerDocument!.createElementNS("http://www.w3.org/2000/svg", "g");
  g.setAttribute(EDIT_HANDLE_ATTR, "");
  g.setAttribute("id", nextHandleId());
  parent.insertBefore(g, el);
  g.appendChild(el);
}

function isWrappable(el: Element): boolean {
  if (NON_WRAPPABLE_TAGS.has(el.localName)) return false;
  return KIND_BY_TAG[el.localName]?.selectable === true;
}

function readDimensions(svg: SVGSVGElement): { w: number; h: number } | null {
  const viewBox = svg.getAttribute("viewBox");
  if (viewBox) {
    const parts = viewBox.split(/[\s,]+/).map((p) => p.trim()).filter(Boolean);
    if (parts.length === 4) {
      const w = Number(parts[2]);
      const h = Number(parts[3]);
      if (Number.isFinite(w) && Number.isFinite(h) && w > 0 && h > 0) {
        return { w, h };
      }
    }
  }
  const w = stripUnit(svg.getAttribute("width"));
  const h = stripUnit(svg.getAttribute("height"));
  if (Number.isFinite(w) && Number.isFinite(h) && (w as number) > 0 && (h as number) > 0) {
    return { w: w as number, h: h as number };
  }
  return null;
}

function stripUnit(value: string | null): number | null {
  if (!value) return null;
  const n = Number(value.replace(/[a-z%]+$/i, ""));
  return Number.isFinite(n) && n > 0 ? n : null;
}

function ensureViewBox(svg: SVGSVGElement, intrinsic: { w: number; h: number } | null): void {
  if (svg.getAttribute("viewBox")) return;
  if (intrinsic) {
    svg.setAttribute("viewBox", `0 0 ${intrinsic.w} ${intrinsic.h}`);
  } else {
    // Sensible fallback so coordinate math is at least internally
    // consistent. Users can edit the root attrs later if they want to
    // change the canvas size.
    svg.setAttribute("viewBox", "0 0 300 150");
    if (!svg.getAttribute("width")) svg.setAttribute("width", "300");
    if (!svg.getAttribute("height")) svg.setAttribute("height", "150");
  }
}
