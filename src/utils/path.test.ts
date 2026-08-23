import { describe, expect, it } from 'vitest';
import {
  areFilePathsEqual,
  getBaseName,
  getDirName,
  getParentDirPath,
  getRelativePath,
  isAbsoluteFilePath,
  isPathInside,
  joinPath,
  normalizeDirPath,
  resolveWorkspaceFilePath,
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

    it('preserves filesystem roots and a UNC prefix', () => {
      expect(normalizeDirPath('/')).toBe('/');
      expect(normalizeDirPath('C:\\')).toBe('C:/');
      expect(normalizeDirPath('\\\\server\\share\\')).toBe('//server/share');
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

    it('uses Windows case-insensitive containment and handles filesystem roots', () => {
      expect(isPathInside('C:\\Work', 'c:/work/Paper.docx')).toBe(true);
      expect(isPathInside('/', '/tmp/paper.docx')).toBe(true);
      expect(isPathInside('C:\\', 'c:/tmp/paper.docx')).toBe(true);
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

    it('matches mixed-case Windows drive paths and preserves child casing', () => {
      expect(getRelativePath('C:\\Work', 'c:/work/Reports/Paper.docx')).toBe(
        'Reports/Paper.docx',
      );
      expect(getRelativePath('C:\\Work', 'c:/WORK')).toBe('');
    });

    it('matches mixed-case UNC paths and preserves child casing', () => {
      expect(
        getRelativePath(
          '\\\\Server\\Share\\Workspace',
          '//server/share/workspace/Research/Paper.docx',
        ),
      ).toBe('Research/Paper.docx');
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
      expect(getDirName('/paper.docx')).toBe('/');
      expect(getDirName('C:\\paper.docx')).toBe('C:/');
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

    it('handles mixed-case Windows drive roots without duplicating the absolute path', () => {
      expect(getParentDirPath('c:\\work\\Reports\\Paper.docx', 'C:\\Work')).toBe(
        'C:/Work/Reports',
      );
      expect(getParentDirPath('c:\\WORK\\Paper.docx', 'C:\\Work')).toBe('C:/Work');
    });

    it('handles mixed-case UNC roots without duplicating the absolute path', () => {
      expect(
        getParentDirPath(
          '\\\\server\\share\\workspace\\Research\\Paper.docx',
          '\\\\Server\\Share\\Workspace',
        ),
      ).toBe('//Server/Share/Workspace/Research');
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

    it('does not turn a POSIX root join into a UNC path', () => {
      expect(joinPath('/', 'tmp', 'paper.docx')).toBe('/tmp/paper.docx');
      expect(joinPath('C:\\', 'tmp')).toBe('C:/tmp');
    });
  });
});

describe('AI/file-event path resolution', () => {
  it('resolves a workspace-relative tool path to the absolute tab path', () => {
    expect(resolveWorkspaceFilePath('reports/paper.docx', 'C:\\Work\\论文')).toBe(
      'C:/Work/论文/reports/paper.docx',
    );
  });

  it('leaves POSIX, drive and UNC absolute paths unchanged apart from separators', () => {
    expect(resolveWorkspaceFilePath('/tmp/paper.docx', '/workspace')).toBe('/tmp/paper.docx');
    expect(resolveWorkspaceFilePath('D:\\docs\\paper.docx', 'C:\\Work')).toBe(
      'D:/docs/paper.docx',
    );
    expect(resolveWorkspaceFilePath('\\\\server\\share\\paper.docx', 'C:\\Work')).toBe(
      '//server/share/paper.docx',
    );
    expect(isAbsoluteFilePath('/tmp/a')).toBe(true);
    expect(isAbsoluteFilePath('C:/tmp/a')).toBe(true);
    expect(isAbsoluteFilePath('tmp/a')).toBe(false);
  });

  it('compares Windows paths case-insensitively but preserves POSIX case', () => {
    expect(areFilePathsEqual('C:\\Work\\Paper.DOCX', 'c:/work/paper.docx')).toBe(true);
    expect(areFilePathsEqual('/Work/Paper.docx', '/work/paper.docx')).toBe(false);
  });
});
