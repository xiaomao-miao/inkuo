import { describe, expect, it } from 'vitest';
import { planFileDrop } from './fileDropPlan';

describe('planFileDrop', () => {
  it('opens a dropped file parent as the workspace on the welcome screen', () => {
    expect(planFileDrop([
      { path: 'C:/docs/paper.docx', isDirectory: false },
    ], null)).toEqual({
      workspaceToOpen: 'C:/docs',
      filesToOpen: ['C:/docs/paper.docx'],
      skippedPaths: [],
    });
  });

  it('keeps the current workspace when files are dropped into an open window', () => {
    expect(planFileDrop([
      { path: '/workspace/paper.docx', isDirectory: false },
    ], '/workspace')).toEqual({
      workspaceToOpen: null,
      filesToOpen: ['/workspace/paper.docx'],
      skippedPaths: [],
    });
  });

  it('prefers the first dropped directory as a workspace and still opens files', () => {
    expect(planFileDrop([
      { path: '/workspace', isDirectory: true },
      { path: '/workspace/paper.docx', isDirectory: false },
    ], '/old')).toEqual({
      workspaceToOpen: '/workspace',
      filesToOpen: ['/workspace/paper.docx'],
      skippedPaths: [],
    });
  });

  it('switches to the first external file parent and skips other workspaces', () => {
    expect(planFileDrop([
      { path: '/current/inside.docx', isDirectory: false },
      { path: '/other/a.docx', isDirectory: false },
      { path: '/third/b.docx', isDirectory: false },
    ], '/current')).toEqual({
      workspaceToOpen: '/other',
      filesToOpen: ['/other/a.docx'],
      skippedPaths: ['/current/inside.docx', '/third/b.docx'],
    });
  });

  it('only opens files inside the dropped directory workspace', () => {
    expect(planFileDrop([
      { path: '/target', isDirectory: true },
      { path: '/second', isDirectory: true },
      { path: '/target/in.docx', isDirectory: false },
      { path: '/outside.docx', isDirectory: false },
    ], '/current')).toEqual({
      workspaceToOpen: '/target',
      filesToOpen: ['/target/in.docx'],
      skippedPaths: ['/second', '/outside.docx'],
    });
  });
});
