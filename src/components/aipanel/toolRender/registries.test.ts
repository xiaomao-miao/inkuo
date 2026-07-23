// Unit tests for the display-name / tool-set registries.

import { describe, expect, it } from 'vitest';

import {
  COMPACT_TOOLS,
  FILE_MODIFICATION_TOOLS,
  PREVIEW_STRING_KEYS,
  extractFileNameFromPath,
  getExpertDisplayName,
  getToolDisplayName,
  isFileModificationTool,
} from './registries';

describe('registries', () => {
  describe('getToolDisplayName', () => {
    it('returns the Chinese label for a known tool', () => {
      expect(getToolDisplayName('read_file')).toBe('读取文件');
      expect(getToolDisplayName('create_excel')).toBe('创建 Excel');
      expect(getToolDisplayName('delegate_to')).toBe('委派子代理');
    });

    it('falls back to the raw name for unknown tools', () => {
      expect(getToolDisplayName('mystery_tool')).toBe('mystery_tool');
      expect(getToolDisplayName('')).toBe('');
    });
  });

  describe('getExpertDisplayName', () => {
    it('returns the Chinese label for a known expert', () => {
      expect(getExpertDisplayName('office_word_expert')).toBe('Word 文档专家');
      expect(getExpertDisplayName('researcher')).toBe('调研员');
    });

    it('falls back to the raw name for unknown experts', () => {
      expect(getExpertDisplayName('nope')).toBe('nope');
    });
  });

  describe('isFileModificationTool', () => {
    it('flags all known file-mutating tools', () => {
      expect(isFileModificationTool('write_file')).toBe(true);
      expect(isFileModificationTool('edit_file')).toBe(true);
      expect(isFileModificationTool('create_word_doc')).toBe(true);
      expect(isFileModificationTool('modify_excel')).toBe(true);
      expect(isFileModificationTool('create_excel')).toBe(true);
      expect(isFileModificationTool('create_pptx')).toBe(true);
    });

    it('returns false for read-only tools', () => {
      expect(isFileModificationTool('read_file')).toBe(false);
      expect(isFileModificationTool('list_dir')).toBe(false);
      expect(isFileModificationTool('grep')).toBe(false);
    });
  });

  describe('extractFileNameFromPath', () => {
    it('extracts the file name from a POSIX path', () => {
      expect(extractFileNameFromPath('/home/me/doc.md')).toBe('doc.md');
    });

    it('extracts the file name from a Windows path', () => {
      expect(extractFileNameFromPath('C:\\Users\\me\\report.docx')).toBe('report.docx');
    });

    it('returns the input unchanged when there is no separator', () => {
      expect(extractFileNameFromPath('just-a-name.md')).toBe('just-a-name.md');
    });

    it('handles falsy inputs as null', () => {
      expect(extractFileNameFromPath(undefined)).toBeNull();
      expect(extractFileNameFromPath(null)).toBeNull();
      expect(extractFileNameFromPath('')).toBeNull();
    });

    it('handles a trailing separator by returning the original path', () => {
      expect(extractFileNameFromPath('/')).toBe('/');
    });
  });

  describe('set membership', () => {
    it('COMPACT_TOOLS contains exactly the compact-card tools', () => {
      const expected = [
        'list_dir',
        'glob',
        'grep',
        'read_file',
        'read_office_file',
        'create_dir',
        'move_file',
      ];
      expect([...COMPACT_TOOLS].sort()).toEqual([...expected].sort());
    });

    it('FILE_MODIFICATION_TOOLS contains exactly the file-mutating tools', () => {
      const expected = [
        'write_file',
        'edit_file',
        'create_word_doc',
        'modify_excel',
        'create_excel',
        'create_pptx',
      ];
      expect([...FILE_MODIFICATION_TOOLS].sort()).toEqual([...expected].sort());
    });

    it('PREVIEW_STRING_KEYS contains the keys that should be shown as raw text', () => {
      expect(PREVIEW_STRING_KEYS.has('content')).toBe(true);
      expect(PREVIEW_STRING_KEYS.has('new_text')).toBe(true);
      expect(PREVIEW_STRING_KEYS.has('pattern')).toBe(true);
      expect(PREVIEW_STRING_KEYS.has('json_content')).toBe(true);
      expect(PREVIEW_STRING_KEYS.has('path')).toBe(false);
    });
  });
});
