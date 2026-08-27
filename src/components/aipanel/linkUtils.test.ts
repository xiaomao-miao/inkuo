import { describe, expect, it } from 'vitest';

import { isExternalHttpLink, isLikelyWorkspacePath, resolveWorkspaceHref, safelyDecodeHref } from './linkUtils';

describe('AI message link helpers', () => {
  it('keeps malformed URI escapes visible instead of throwing', () => {
    expect(safelyDecodeHref('report%2')).toBe('report%2');
    expect(safelyDecodeHref('%E6%B5%8B%E8%AF%95.md')).toBe('测试.md');
  });

  it('recognizes absolute and relative workspace files', () => {
    expect(isLikelyWorkspacePath('/tmp/report.docx')).toBe(true);
    expect(isLikelyWorkspacePath('C:\\Work\\report.docx')).toBe(true);
    expect(isLikelyWorkspacePath('./src/App.tsx')).toBe(true);
    expect(isLikelyWorkspacePath('README.md')).toBe(true);
  });

  it('does not reinterpret web, mail, or anchor links as files', () => {
    expect(isExternalHttpLink('HTTPS://example.com')).toBe(true);
    expect(isLikelyWorkspacePath('https://example.com/a.md')).toBe(false);
    expect(isLikelyWorkspacePath('mailto:user@example.com')).toBe(false);
    expect(isLikelyWorkspacePath('#section')).toBe(false);
  });

  it('resolves relative files without corrupting Windows absolute paths', () => {
    expect(resolveWorkspaceHref('./docs/report.md', '/work/project')).toBe('/work/project/docs/report.md');
    expect(resolveWorkspaceHref('C:\\Work\\report.docx', 'D:\\Project')).toBe('C:\\Work\\report.docx');
    expect(resolveWorkspaceHref('docs\\report.docx', 'D:\\Project')).toBe('D:\\Project\\docs\\report.docx');
  });
});
