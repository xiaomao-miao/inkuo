// Unit tests for the streaming-JSON extractors used by the live tool-call UI.

import { describe, expect, it } from 'vitest';

import {
  extractArrayBody,
  extractFieldFromRaw,
  renderElementsFromRaw,
  renderOperationsFromRaw,
  renderSheetsFromRaw,
  splitArrayEntries,
} from './streamingExtractors';

describe('streamingExtractors', () => {
  describe('extractFieldFromRaw', () => {
    it('extracts simple string values', () => {
      expect(extractFieldFromRaw('{"path": "/a/b.md"}', 'path')).toBe('/a/b.md');
    });

    it('decodes escaped string sequences via JSON.parse', () => {
      // The regex picks up the raw bytes, then JSON.parse unescapes \n
      expect(extractFieldFromRaw('{"text": "line1\\nline2"}', 'text')).toBe(
        'line1\nline2',
      );
    });

    it('returns the raw regex group on a JSON.parse failure', () => {
      // An unterminated escape makes JSON.parse throw; the helper should
      // still return whatever the regex captured.
      expect(extractFieldFromRaw('{"text": "bad \\u escape', 'text')).toBe(
        'bad \\u escape',
      );
    });

    it('extracts numeric values', () => {
      expect(extractFieldFromRaw('{"top_k": 7}', 'top_k')).toBe('7');
      expect(extractFieldFromRaw('{"rate": 1.5}', 'rate')).toBe('1.5');
    });

    it('extracts boolean values', () => {
      expect(extractFieldFromRaw('{"flag": true}', 'flag')).toBe('true');
      expect(extractFieldFromRaw('{"flag": false}', 'flag')).toBe('false');
    });

    it('returns an array-start placeholder', () => {
      expect(extractFieldFromRaw('{"items": [', 'items')).toBe('[…正在生成…]');
    });

    it('returns an object-start placeholder', () => {
      expect(extractFieldFromRaw('{"meta": {', 'meta')).toBe('{…}');
    });

    it('returns null when the key is missing', () => {
      expect(extractFieldFromRaw('{"path": "/a"}', 'missing')).toBeNull();
    });
  });

  describe('extractArrayBody', () => {
    it('returns the body after the opening bracket', () => {
      expect(extractArrayBody('{"items": [{ "a": 1', 'items')).toBe('{ "a": 1');
    });

    it('returns null when the key is absent', () => {
      expect(extractArrayBody('{"other": [1,2]}', 'items')).toBeNull();
    });
  });

  describe('splitArrayEntries', () => {
    it('splits a flat array of objects on top-level commas', () => {
      const body = '{ "a": 1 }, { "b": 2 }, { "c": 3 }';
      expect(splitArrayEntries(body)).toEqual([
        '{ "a": 1 }',
        ' { "b": 2 }',
        ' { "c": 3 }',
      ]);
    });

    it('does not split on commas inside nested objects', () => {
      const body = '{ "a": { "x": 1, "y": 2 } }, { "b": 3 }';
      const entries = splitArrayEntries(body);
      expect(entries.length).toBe(2);
      expect(entries[0]).toBe('{ "a": { "x": 1, "y": 2 } }');
      expect(entries[1]).toBe(' { "b": 3 }');
    });

    it('handles commas inside string values', () => {
      const body = '{ "text": "a, b, c" }, { "text": "d" }';
      const entries = splitArrayEntries(body);
      expect(entries.length).toBe(2);
      expect(entries[0]).toBe('{ "text": "a, b, c" }');
      expect(entries[1]).toBe(' { "text": "d" }');
    });

    it('handles escaped quotes inside string values', () => {
      const body = '{ "text": "a \\"b\\"" }, { "text": "c" }';
      const entries = splitArrayEntries(body);
      expect(entries.length).toBe(2);
      expect(entries[0]).toBe('{ "text": "a \\"b\\"" }');
      expect(entries[1]).toBe(' { "text": "c" }');
    });

    it('captures the trailing fragment as a final entry', () => {
      const entries = splitArrayEntries('{ "a": 1 }, { "b": ');
      expect(entries.length).toBe(2);
      expect(entries[1]).toBe(' { "b": ');
    });

    it('skips whitespace-only fragments', () => {
      const entries = splitArrayEntries('   ');
      expect(entries).toEqual([]);
    });
  });

  describe('renderElementsFromRaw', () => {
    it('extracts plain text entries', () => {
      const raw = '{"elements": [{"text": "第一段"}, {"text": "第二段"}';
      expect(renderElementsFromRaw(raw, 'elements')).toBe('第一段\n第二段');
    });

    it('falls back to table-header rendering for text-less entries', () => {
      const raw = '{"elements": [{"header": ["列A", "列B"]}]';
      expect(renderElementsFromRaw(raw, 'elements')).toBe('列A | 列B');
    });

    it('skips entries that are still generating', () => {
      const raw = '{"elements": [{"text": "完成"}, {"text": "[…正在生成…]"}';
      expect(renderElementsFromRaw(raw, 'elements')).toBe('完成');
    });

    it('returns null when the array body is absent', () => {
      expect(renderElementsFromRaw('{}', 'elements')).toBeNull();
    });
  });

  describe('renderOperationsFromRaw', () => {
    it('renders modify_cell entries', () => {
      const raw =
        '{"operations": [{"type": "modify_cell", "sheet": "S1", "address": "A1", "formula": "SUM(A2:A5)"}';
      expect(renderOperationsFromRaw(raw, 'operations')).toBe(
        'S1!A1 = =SUM(A2:A5)',
      );
    });

    it('omits the sheet prefix when sheet is still generating', () => {
      const raw =
        '{"operations": [{"type": "modify_cell", "sheet": "[…正在生成…]", "address": "B2"}';
      expect(renderOperationsFromRaw(raw, 'operations')).toBe('B2 = …');
    });

    it('falls back to "<type>…" for unknown types', () => {
      const raw = '{"operations": [{"type": "weird_op"}';
      expect(renderOperationsFromRaw(raw, 'operations')).toBe('weird_op…');
    });

    it('returns null when the array is missing', () => {
      expect(renderOperationsFromRaw('{}', 'operations')).toBeNull();
    });
  });

  describe('renderSheetsFromRaw', () => {
    it('renders sheet name headers', () => {
      const raw = '{"sheets": [{"name": "销售"}';
      expect(renderSheetsFromRaw(raw, 'sheets')).toBe('【销售】');
    });

    it('renders pending cell addresses with placeholders', () => {
      const raw = '{"sheets": [{"name": "销售", "cells": [{"address": "A1"';
      expect(renderSheetsFromRaw(raw, 'sheets')).toBe('【销售】\n  A1…');
    });

    it('returns null when the sheets array is missing', () => {
      expect(renderSheetsFromRaw('{}', 'sheets')).toBeNull();
    });
  });
});
