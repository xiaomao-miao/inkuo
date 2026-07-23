// Single-cell value + style conversion between Rust and FortuneSheet.
//
// These functions operate on a `FortuneCell` / `RustCell` pair and are
// shared by `sheetConversions.ts` (forward + backward full-sheet flow)
// and the SheetJS exporter. Keeping the cell-level mapping separate
// means each function does one thing and can be reasoned about in
// isolation.

import type { Cell as FortuneCell } from '@fortune-sheet/core';

import { alignH, alignV, normaliseColor } from './colorAlignment';
import type { RustCell, RustCellStyle, RustCellValue } from './types';

/** Convert a single Rust cell (value + style + formula) into a FortuneCell. */
export function rustCellToFortune(cell: RustCell): FortuneCell {
  const fortune: FortuneCell = {};

  switch (cell.value.type) {
    case 'empty':
      break;
    case 'int':
      fortune.v = cell.value.value ?? 0;
      fortune.ct = { fa: '0', t: 'n' };
      fortune.m = String(cell.value.value ?? 0);
      break;
    case 'float':
      fortune.v = cell.value.value ?? 0;
      fortune.ct = { fa: 'General', t: 'n' };
      fortune.m = String(cell.value.value ?? 0);
      break;
    case 'datetime': {
      const serial = cell.value.value ?? 0;
      fortune.v = serial;
      const fmt = cell.style?.number_format;
      fortune.ct = {
        fa: fmt && fmt !== 'General' ? fmt : 'yyyy-mm-dd',
        t: 'n',
      };
      fortune.m = String(serial);
      break;
    }
    case 'bool':
      fortune.v = cell.value.value !== 0;
      fortune.m = fortune.v ? 'TRUE' : 'FALSE';
      fortune.ct = { fa: 'General', t: 'g' };
      break;
    case 'string':
      fortune.v = cell.value.value ?? '';
      fortune.m = cell.value.value ?? '';
      fortune.ct = { fa: '@', t: 's' };
      break;
    case 'error':
      fortune.v = `#ERR:${cell.value.value ?? ''}`;
      fortune.m = `#ERR:${cell.value.value ?? ''}`;
      fortune.ct = { fa: 'General', t: 'g' };
      break;
  }

  if (cell.formula) {
    // OOXML <f> has no leading "=", HyperFormula requires it.
    fortune.f = cell.formula.startsWith('=') ? cell.formula : `=${cell.formula}`;
  }

  const s = cell.style;
  if (s) {
    if (s.font_bold) fortune.bl = 1;
    if (s.font_italic) fortune.it = 1;
    if (s.font_size) fortune.fs = s.font_size;
    if (s.font_name) fortune.ff = s.font_name;
    const fc = normaliseColor(s.font_color);
    if (fc) fortune.fc = fc;
    const bg = normaliseColor(s.fill_fg_color);
    if (bg) fortune.bg = bg;
    const h = alignH(s.alignment_h);
    if (h !== undefined) fortune.ht = h;
    const v = alignV(s.alignment_v);
    if (v !== undefined) fortune.vt = v;
    if (s.number_format && s.number_format !== 'General') {
      if (!fortune.ct) fortune.ct = { fa: s.number_format, t: 'g' };
    }
  }

  return fortune;
}

/**
 * Infer a Rust `CellValue` from a FortuneSheet cell value.
 *
 * FortuneSheet formula cells carry both the formula text (`f`) and
 * the computed result (`v`). When HyperFormula hasn't computed yet
 * (`v` is the formula string itself), we treat the value as empty —
 * the formula lives in `cell.formula` and is restored separately.
 */
export function fortuneValueToRust(v: FortuneCell): RustCellValue {
  const hasFormula = typeof v.f === 'string' && v.f.length > 0;
  const raw = v.v;

  // Empty / no value — even for formula cells (formula may produce empty)
  if (raw === undefined || raw === null || raw === '') {
    return { type: 'empty' };
  }

  // Formula that produced a zero/numeric result
  if (hasFormula && typeof raw === 'number' && !isNaN(raw)) {
    if (Number.isInteger(raw)) return { type: 'int', value: raw };
    return { type: 'float', value: raw };
  }

  // Formula that produced a boolean
  if (hasFormula && typeof raw === 'boolean') {
    return { type: 'bool', value: raw ? 1 : 0 };
  }

  // Formula that produced an error string
  if (hasFormula && typeof raw === 'string' && raw.startsWith('#')) {
    return { type: 'error', value: raw };
  }

  // Formula that produced a string result
  if (hasFormula && typeof raw === 'string') {
    if (raw.startsWith('=')) {
      // HyperFormula hasn't run yet — treat as empty; formula text is stored
      // separately in the Rust formula field (handled by the caller).
      return { type: 'empty' };
    }
    return { type: 'string', value: raw };
  }

  // Non-formula cells
  const ct = v.ct;
  if (ct?.t === 's') {
    return { type: 'string', value: String(raw) };
  }
  if (ct?.t === 'n') {
    const num = typeof raw === 'number' ? raw : Number(raw);
    if (isNaN(num)) return { type: 'string', value: String(raw) };
    // Date formats take precedence over int/float: a serial date is
    // numerically an integer, but `yyyy-mm-dd` etc. signal that we
    // should preserve it as a datetime for the round-trip.
    const fa = ct.fa ?? '';
    if (fa.includes('yy') || fa.includes('mm') || fa.includes('dd') || fa.includes('hh')) {
      return { type: 'datetime', value: num };
    }
    if (Number.isInteger(num)) return { type: 'int', value: num };
    return { type: 'float', value: num };
  }
  if (ct?.t === 'b') {
    return { type: 'bool', value: raw ? 1 : 0 };
  }
  if (typeof raw === 'string' && raw.startsWith('=')) {
    return { type: 'string', value: raw };
  }
  return { type: 'string', value: String(raw) };
}

/** Build a Rust cell style from a FortuneSheet cell's style hints. */
export function fortuneStyleToRust(v: FortuneCell): RustCellStyle {
  const style: RustCellStyle = {};
  if (v.bl === 1) style.font_bold = true;
  if (v.it === 1) style.font_italic = true;
  if (v.fs != null) style.font_size = v.fs;
  if (v.ff != null && typeof v.ff === 'string') style.font_name = v.ff;
  if (v.fc != null) style.font_color = v.fc;
  if (v.bg != null) style.fill_fg_color = v.bg;
  if (v.ht !== undefined) {
    switch (v.ht) {
      case 0:
        style.alignment_h = 'center';
        break;
      case 1:
        style.alignment_h = 'left';
        break;
      case 2:
        style.alignment_h = 'right';
        break;
    }
  }
  if (v.vt !== undefined) {
    switch (v.vt) {
      case 0:
        style.alignment_v = 'center';
        break;
      case 1:
        style.alignment_v = 'top';
        break;
      case 2:
        style.alignment_v = 'bottom';
        break;
    }
  }
  if (v.ct?.fa) style.number_format = v.ct.fa;
  return style;
}
