import { describe, expect, it } from 'vitest';
import {
  getBaseName,
  getDirName,
  getRelativePath,
  isPathInside,
  joinPath,
  normalizeDirPath,
} from './path';

describe('normalizeDirPath', () => {
  it('collapses Windows backslashes to forward slashes', () => {
    expect(normalizeDirPath('E:\\文档\\sub')).toBe('E:/文档/sub');
  });

  it('preserves forward slashes', () => {
    expect(normalizeDirPath('/foo/bar')).toBe('/foo/bar');
  });

  it('strips trailing separators', () => {
    expect(normalizeDirPath('/foo/bar/')).toBe('/foo/bar');
    expect(normalizeDirPath('/foo/bar///')).toBe('/foo/bar');
  });

  it('returns an empty string for falsy input', () => {
    expect(normalizeDirPath('')).toBe('');
  });
});

describe('isPathInside', () => {
  it('returns true when child equals parent', () => {
    expect(isPathInside('E:\\文档', 'E:\\文档')).toBe(true);
  });

  it('returns true for a nested child regardless of separator', () => {
    expect(isPathInside('E:\\文档', 'E:\\文档\\sub\\file.md')).toBe(true);
    expect(isPathInside('E:/文档', 'E:\\文档\\sub\\file.md')).toBe(true);
  });

  it('returns false for siblings with a shared prefix', () => {
    expect(isPathInside('E:\\文档', 'E:\\文档2')).toBe(false);
  });

  it('returns false for empty inputs', () => {
    expect(isPathInside('', 'foo')).toBe(false);
    expect(isPathInside('foo', '')).toBe(false);
  });
});

describe('getRelativePath', () => {
  it('strips the parent prefix using normalized separators', () => {
    expect(getRelativePath('E:\\文档', 'E:\\文档\\a\\b.md')).toBe('a/b.md');
    expect(getRelativePath('E:/文档', 'E:\\文档\\a')).toBe('a');
  });

  it('returns an empty string when child equals parent', () => {
    expect(getRelativePath('E:\\文档', 'E:\\文档')).toBe('');
  });

  it('returns the child unchanged when it is not under parent', () => {
    expect(getRelativePath('E:\\文档', 'D:\\other\\file.md')).toBe('D:/other/file.md');
  });
});

describe('getBaseName', () => {
  it('handles Windows paths', () => {
    expect(getBaseName('E:\\文档\\WordAI方法文档管理规范.docx')).toBe('WordAI方法文档管理规范.docx');
  });

  it('handles POSIX paths', () => {
    expect(getBaseName('/foo/bar/baz.md')).toBe('baz.md');
  });

  it('returns the input when there is no separator', () => {
    expect(getBaseName('README.md')).toBe('README.md');
  });

  it('returns an empty string for empty input', () => {
    expect(getBaseName('')).toBe('');
  });
});

describe('getDirName', () => {
  it('returns the parent directory for a nested path', () => {
    expect(getDirName('E:\\文档\\sub\\file.md')).toBe('E:/文档/sub');
  });

  it('returns an empty string for a top-level path', () => {
    expect(getDirName('README.md')).toBe('');
  });
});

describe('joinPath', () => {
  it('joins segments with forward slashes', () => {
    expect(joinPath('E:\\文档', 'sub', 'nested')).toBe('E:/文档/sub/nested');
  });

  it('ignores nullish and empty segments', () => {
    expect(joinPath('E:\\文档', '', 'sub', undefined)).toBe('E:/文档/sub');
  });
});