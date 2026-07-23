import { describe, expect, it } from 'vitest';
import {
  getBaseName,
  getDirName,
  getParentDirPath,
  getRelativePath,
  isPathInside,
  joinPath,
  normalizeDirPath,
} from './path';

describe('utils/path', () => {
  describe('normalizeDirPath', () => {
    it('collapses backslashes to forward slashes', () => {
      expect(normalizeDirPath('E:\\foo\\bar')).toBe('E:/foo/bar');
    });

    it('strips trailing slashes', () => {
      expect(normalizeDirPath('/foo/bar/')).toBe('/foo/bar');
      expect(normalizeDirPath('/foo/bar///')).toBe('/foo/bar');
    });

    it('returns empty string for empty / falsy input', () => {
      expect(normalizeDirPath('')).toBe('');
      expect(normalizeDirPath(undefined as unknown as string)).toBe('');
    });

    it('treats mixed separators as a single slash', () => {
      expect(normalizeDirPath('a/b\\c/d')).toBe('a/b/c/d');
    });
  });

  describe('isPathInside', () => {
    it('treats the parent as inside itself', () => {
      expect(isPathInside('/root', '/root')).toBe(true);
    });

    it('matches nested descendants', () => {
      expect(isPathInside('/root', '/root/sub/file.md')).toBe(true);
    });

    it('rejects siblings with shared prefixes', () => {
      // /root2 is NOT inside /root even though it shares the prefix.
      expect(isPathInside('/root', '/root2/file.md')).toBe(false);
    });

    it('is separator-agnostic', () => {
      expect(isPathInside('E:\\foo', 'E:/foo/bar')).toBe(true);
    });
  });

  describe('getRelativePath', () => {
    it('strips the parent from the front of the child', () => {
      expect(getRelativePath('/root', '/root/a/b.md')).toBe('a/b.md');
    });

    it('returns empty when child equals parent', () => {
      expect(getRelativePath('/root', '/root')).toBe('');
    });

    it('returns the child verbatim when there is no common prefix', () => {
      expect(getRelativePath('/root', '/other/path.md')).toBe('/other/path.md');
    });
  });

  describe('getBaseName', () => {
    it('returns the deepest path component', () => {
      expect(getBaseName('/foo/bar/baz.md')).toBe('baz.md');
      expect(getBaseName('C:\\foo\\bar.md')).toBe('bar.md');
    });

    it('returns empty string for empty input', () => {
      expect(getBaseName('')).toBe('');
    });
  });

  describe('getDirName', () => {
    it('returns the parent directory', () => {
      expect(getDirName('/foo/bar/baz.md')).toBe('/foo/bar');
    });

    it('returns empty string when there is no parent', () => {
      expect(getDirName('baz.md')).toBe('');
      expect(getDirName('')).toBe('');
    });
  });

  describe('getParentDirPath', () => {
    it('returns the workspace root for a direct child', () => {
      expect(getParentDirPath('/root/a.md', '/root')).toBe('/root');
    });

    it('walks up to the right ancestor for nested files', () => {
      expect(getParentDirPath('/root/sub/nested/c.md', '/root')).toBe('/root/sub/nested');
    });

    it('returns null when the workspace root is empty', () => {
      expect(getParentDirPath('/root/a.md', '')).toBeNull();
    });

    it('is separator-agnostic', () => {
      expect(getParentDirPath('E:\\root\\sub\\file.md', 'E:/root')).toBe('E:/root/sub');
    });
  });

  describe('joinPath', () => {
    it('joins parent and segments with forward slashes', () => {
      expect(joinPath('/root', 'sub', 'nested')).toBe('/root/sub/nested');
    });

    it('skips empty / nullish segments', () => {
      expect(joinPath('/root', '', null, undefined, 'sub')).toBe('/root/sub');
    });

    it('normalizes the joined result', () => {
      expect(joinPath('/root/', 'sub', 'nested')).toBe('/root/sub/nested');
    });
  });
});
