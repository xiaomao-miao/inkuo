// Unit tests for the small path helpers used by `menuBuilders.tsx`.
//
// These wrap `utils/path` (which already has its own tests) but add
// menu-specific behavior around extension splitting and dedup-name
// generation. We don't replicate the underlying path normalization
// here — `utils/path` is the source of truth, and these tests focus
// on the deltas that pathHelpers adds on top.

import { describe, expect, it } from 'vitest';

import {
  basename,
  fileExtension,
  fileStem,
  joinPath,
  parentPath,
  uniqueSiblingName,
} from './pathHelpers';

describe('basename / parentPath / joinPath (delegating to utils/path)', () => {
  it('basename returns the last component for POSIX paths', () => {
    expect(basename('/home/me/file.md')).toBe('file.md');
  });

  it('basename returns the last component for Windows paths', () => {
    expect(basename('C:\\Users\\me\\report.docx')).toBe('report.docx');
  });

  it('basename returns the input when no separator is present', () => {
    expect(basename('just-a-name.md')).toBe('just-a-name.md');
  });

  it('parentPath returns the POSIX directory portion', () => {
    expect(parentPath('/home/me/file.md')).toBe('/home/me');
  });

  it('parentPath returns the directory portion for a Windows path', () => {
    // The underlying `utils/path` normalizes separators to '/', but
    // preserves a Windows drive letter (`C:`). We assert the platform
    // behavior rather than a fictitious Windows form.
    expect(parentPath('C:\\Users\\me\\report.docx')).toBe('C:/Users/me');
  });

  it('parentPath returns empty string for a bare filename', () => {
    expect(parentPath('file.md')).toBe('');
  });

  it('joinPath joins POSIX segments', () => {
    expect(joinPath('/home/me', 'doc.md')).toBe('/home/me/doc.md');
  });

  it('joinPath joins segments using the normalized separator', () => {
    // `utils/path` normalizes the separator but preserves a Windows
    // drive letter (`C:`); the joined path therefore has `/` between
    // segments while still anchored at the drive root.
    expect(joinPath('C:\\Users', 'doc.md')).toBe('C:/Users/doc.md');
  });

  it('joinPath returns the name when parent is empty', () => {
    expect(joinPath('', 'doc.md')).toBe('doc.md');
  });
});

describe('fileStem / fileExtension', () => {
  it('splits simple filenames into stem + extension', () => {
    expect(fileStem('doc.md')).toBe('doc');
    expect(fileExtension('doc.md')).toBe('.md');
  });

  it('treats the last dot as the extension boundary', () => {
    expect(fileStem('archive.tar.gz')).toBe('archive.tar');
    expect(fileExtension('archive.tar.gz')).toBe('.gz');
  });

  it('treats hidden files / dotfiles as having no extension', () => {
    expect(fileStem('.gitignore')).toBe('.gitignore');
    expect(fileExtension('.gitignore')).toBe('');
  });

  it('returns the whole name as stem when there is no dot', () => {
    expect(fileStem('README')).toBe('README');
    expect(fileExtension('README')).toBe('');
  });
});

describe('uniqueSiblingName', () => {
  it('inserts a -<timestamp> suffix between the stem and the extension', () => {
    const before = Date.now();
    const result = uniqueSiblingName('/home/me', 'doc.md');
    const after = Date.now();
    const m = result.match(/^(\/home\/me)\/doc-(\d+)(\.md)$/);
    expect(m).not.toBeNull();
    const ts = Number(m![2]);
    expect(ts).toBeGreaterThanOrEqual(before);
    expect(ts).toBeLessThanOrEqual(after);
  });

  it('appends the suffix when there is no extension', () => {
    const m = uniqueSiblingName('/home/me', 'README').match(/^(\/home\/me)\/README-(\d+)$/);
    expect(m).not.toBeNull();
  });
});
