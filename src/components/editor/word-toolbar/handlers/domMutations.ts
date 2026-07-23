// Pure document-model mutation helpers used by the Word toolbar's
// page-color / header-footer / watermark handlers.
//
// Each function takes a document model (the same JSON shape the
// editor handle's `getDocument` / `loadDocument` round-trips) and
// returns a new model. No React, no ProseMirror — so they're easy to
// unit-test and reason about in isolation.
//
// All entries intentionally take `notify` (a thin error-reporter)
// so the calling hook can stay declarative and the helper itself
// stays framework-free. The helpers return either the next document
// model or `null` when the operation was a no-op (e.g. clearing the
// page color when none was set) so the hook can decide what to do.

export type Notify = (kind: 'error' | 'info', message: string) => void;

// ── Page color ────────────────────────────────────────────────────────────────

/**
 * Shape of `doc.body.finalSectionProperties.background` used by the
 * editor handle. Kept structural (unknown fields widened to `unknown`)
 * because we round-trip through `JSON.parse(JSON.stringify(...))`
 * before re-loading.
 */
export interface DocBody {
  finalSectionProperties?: {
    background?: { color?: { rgb?: string } };
    headerReferences?: Array<{ type: string; rId: string }>;
    footerReferences?: Array<{ type: string; rId: string }>;
    titlePage?: boolean;
  };
}

/** Top-level document model — broad enough to round-trip JSON losslessly. */
export interface DocModel {
  body?: DocBody;
  headers?: Map<string, unknown> | Record<string, unknown>;
  footers?: Map<string, unknown> | Record<string, unknown>;
  [key: string]: unknown;
}

/** Normalize a hex color: strip leading `#`, upper-case. */
function normalizeHexColor(color: string): string {
  return color.replace(/^#/, '').toUpperCase();
}

/**
 * Apply (or clear) the page background color on `doc`. Returns the
 * next document model or `null` when the requested operation is a
 * no-op (`color` empty + no current background).
 *
 * `color` of `'none'` / `''` is treated as "clear".
 */
export function applyPageColor(
  doc: DocModel,
  color: string,
): DocModel | null {
  const next = JSON.parse(JSON.stringify(doc)) as DocModel;
  if (!next.body) next.body = {};
  if (!next.body.finalSectionProperties) next.body.finalSectionProperties = {};
  if (color === 'none' || !color) {
    if (!next.body.finalSectionProperties.background) return null;
    delete next.body.finalSectionProperties.background;
    return next;
  }
  next.body.finalSectionProperties.background = {
    color: { rgb: normalizeHexColor(color) },
  };
  return next;
}

// ── Header / footer ───────────────────────────────────────────────────────────

export type HeaderFooterKind = 'header' | 'footer';

export interface HeaderFooterApply {
  text: string;
  alignment?: 'left' | 'center' | 'right';
  /** When true, append a `PAGE` field after the text run. */
  includePageNumber?: boolean;
  /**
   * When true, suppress the special "first page" header/footer so
   * the new part covers page 1 as well. Matches Word's "different
   * first page" toggle.
   */
  insertBeforeFirstPage?: boolean;
}

/**
 * Build the runs array for a header/footer paragraph. Pure — no
 * dependencies on the doc model.
 */
export function buildHeaderFooterRuns(cfg: HeaderFooterApply): Array<Record<string, unknown>> {
  const runs: Array<Record<string, unknown>> = [];
  if (cfg.text) {
    runs.push({ text: cfg.text, type: 'run' });
  }
  if (cfg.includePageNumber) {
    if (runs.length > 0) {
      runs.push({ text: ' ', type: 'run' });
    }
    runs.push({ text: 'PAGE', type: 'field', fieldType: 'PAGE' });
  }
  return runs;
}

/**
 * Apply (or replace) the header / footer described by `cfg` into
 * `doc`. Returns the next document model or `null` when the requested
 * part is empty (e.g. `cfg.text` empty + `includePageNumber` false),
 * since writing a blank header/footer is treated as a no-op.
 */
export function applyHeaderFooter(
  doc: DocModel,
  kind: HeaderFooterKind,
  cfg: HeaderFooterApply,
): DocModel | null {
  const runs = buildHeaderFooterRuns(cfg);
  if (runs.length === 0) return null;

  const { headers, footers, ...rest } = doc;
  const next = JSON.parse(JSON.stringify(rest)) as DocModel;
  if (!next.body) next.body = {};
  if (!next.body.finalSectionProperties) next.body.finalSectionProperties = {};

  const rId = `rId${kind}-${Date.now()}`;
  const newPart = {
    type: kind,
    hdrFtrType: 'default',
    content: [
      {
        type: 'paragraph',
        alignment: cfg.alignment,
        runs,
      },
    ],
  };

  // Round-trip through entries to handle both Map and plain-object
  // shapes — the editor core's `getDocument()` returns Maps, but the
  // `loadDocument()` payload accepts plain objects.
  const existing = kind === 'header' ? headers : footers;
  const partsMap = new Map<string, unknown>(
    existing instanceof Map
      ? Array.from(existing.entries())
      : Object.entries(existing ?? {}),
  );
  partsMap.set(rId, newPart);

  const refsKey = kind === 'header' ? 'headerReferences' : 'footerReferences';
  const refs = next.body.finalSectionProperties[refsKey] ?? [];
  refs.push({ type: 'default', rId });
  next.body.finalSectionProperties[refsKey] = refs;
  if (kind === 'header') next.headers = partsMap;
  else next.footers = partsMap;

  if (cfg.insertBeforeFirstPage) {
    next.body.finalSectionProperties.titlePage = false;
    const filtered = refs.filter((r) => r.type !== 'first');
    next.body.finalSectionProperties[refsKey] = filtered;
  }

  return next;
}

// ── Watermark apply ───────────────────────────────────────────────────────────

export interface WatermarkApplyConfig {
  text: string;
  font: string;
  color: string;
  semitransparent: boolean;
  layout: 'diagonal' | 'horizontal';
  fontSize: number;
}

/**
 * Build the watermark spec object passed to ProseMirror's
 * `setWatermark` command. Centralized so the toolbar handler and
 * any future programmatic caller agree on the shape.
 */
export function buildWatermarkSpec(cfg: WatermarkApplyConfig): Record<string, unknown> {
  return {
    kind: 'text',
    text: cfg.text,
    font: cfg.font,
    color: cfg.color,
    semitransparent: cfg.semitransparent,
    layout: cfg.layout,
    fontSize: cfg.fontSize,
  };
}