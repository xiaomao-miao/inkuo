import { useEditorStore, useSidebarStore, type SearchResult } from '../../store';

function lineStartOffset(content: string, lineNumber: number): number {
  if (lineNumber <= 1) return 0;

  let currentLine = 1;
  for (let i = 0; i < content.length; i += 1) {
    if (currentLine === lineNumber) {
      return i;
    }
    if (content[i] === '\n') {
      currentLine += 1;
    }
  }

  return content.length;
}

export function openKnowledgeReference(result: Pick<SearchResult, 'filePath' | 'documentTitle' | 'startLine' | 'endLine'>): void {
  if (!result.filePath || !result.filePath.trim()) {
    return;
  }

  const fileName = result.filePath.split('/').pop() || '未命名文档';
  useSidebarStore.getState().openWorkspaceFile(result.filePath, {
    name: result.documentTitle || fileName,
  });

  const startLine = result.startLine;
  if (!startLine) return;

  const resolvedPath = (() => {
    const sidebarState = useSidebarStore.getState();
    const directMatch = sidebarState.files.find((file) => !file.is_dir && file.path === result.filePath);
    if (directMatch) return directMatch.path;

    const workspacePath = sidebarState.workspacePath;
    if (workspacePath && result.filePath.startsWith(workspacePath)) {
      const relativePath = result.filePath.slice(workspacePath.length).replace(/^\//, '');
      const relativeMatch = sidebarState.files.find((file) => !file.is_dir && file.path === relativePath);
      if (relativeMatch) return relativeMatch.path;
      return relativePath;
    }

    return result.filePath;
  })();

  const applySelection = () => {
    const docState = useEditorStore.getState().documentContents[resolvedPath];
    if (!docState || !docState.metadata.content) return false;

    const content = docState.metadata.content;
    const from = lineStartOffset(content, startLine);
    const to = lineStartOffset(content, (result.endLine ?? startLine) + 1);
    useEditorStore.getState().setSelection(resolvedPath, { from, to });
    return true;
  };

  if (applySelection()) return;

  const pollInterval = window.setInterval(() => {
    if (applySelection()) {
      window.clearInterval(pollInterval);
    }
  }, 100);

  window.setTimeout(() => window.clearInterval(pollInterval), 5000);
}

export function buildKnowledgeReferenceHref(result: Pick<SearchResult, 'filePath' | 'startLine' | 'endLine'>): string {
  const params = new URLSearchParams();
  params.set('path', result.filePath || '');
  if (result.startLine) params.set('startLine', String(result.startLine));
  if (result.endLine) params.set('endLine', String(result.endLine));
  return `inkuo://knowledge-reference?${params.toString()}`;
}
