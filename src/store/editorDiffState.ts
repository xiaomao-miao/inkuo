import type { DiffHunk } from '../types';

interface DiffContext {
  content: string;
  diffHunks: DiffHunk[];
  diffOriginalText: string;
  diffOriginalOffset: number;
}

interface DiffResolution {
  content: string;
  diffHunks: DiffHunk[];
  diffOriginalText: string;
  diffOriginalOffset: number;
  isDiffMode: boolean;
  isDirty?: boolean;
}

function applyHunkToText(originalText: string, hunk: DiffHunk): string {
  const oldContent = hunk.changes
    .filter((change) => change.tag !== 'insert')
    .map((change) => change.content)
    .join('');
  const newContent = hunk.changes
    .filter((change) => change.tag !== 'delete')
    .map((change) => change.content)
    .join('');

  const oldIndex = originalText.indexOf(oldContent, hunk.old_offset);
  if (oldIndex === -1 || oldIndex !== hunk.old_offset) {
    return (
      originalText.slice(0, hunk.old_offset) +
      newContent +
      originalText.slice(hunk.old_offset + oldContent.length)
    );
  }

  return (
    originalText.slice(0, hunk.old_offset) +
    newContent +
    originalText.slice(hunk.old_offset + oldContent.length)
  );
}

function applyAllHunksToText(originalText: string, hunks: DiffHunk[]): string {
  const sortedHunks = [...hunks].sort((left, right) => right.old_offset - left.old_offset);
  return sortedHunks.reduce((text, hunk) => applyHunkToText(text, hunk), originalText);
}

export function applySelectedHunk(context: DiffContext, hunkId: string): DiffResolution | null {
  const hunk = context.diffHunks.find((candidate) => candidate.id === hunkId);
  if (!hunk) {
    return null;
  }

  const updatedFragment = applyHunkToText(context.diffOriginalText, hunk);
  const fragmentEndOffset = context.diffOriginalOffset + context.diffOriginalText.length;
  const remainingHunks = context.diffHunks.filter((candidate) => candidate.id !== hunkId);

  return {
    content:
      context.content.slice(0, context.diffOriginalOffset) +
      updatedFragment +
      context.content.slice(fragmentEndOffset),
    diffHunks: remainingHunks,
    diffOriginalText: context.diffOriginalText,
    diffOriginalOffset: context.diffOriginalOffset,
    isDiffMode: remainingHunks.length > 0,
    isDirty: true,
  };
}

export function rejectSelectedHunk(context: DiffContext, hunkId: string): DiffResolution {
  const remainingHunks = context.diffHunks.filter((candidate) => candidate.id !== hunkId);

  return {
    content: context.content,
    diffHunks: remainingHunks,
    diffOriginalText: context.diffOriginalText,
    diffOriginalOffset: context.diffOriginalOffset,
    isDiffMode: remainingHunks.length > 0,
  };
}

export function applyRemainingHunks(context: DiffContext): DiffResolution {
  const updatedFragment = applyAllHunksToText(context.diffOriginalText, context.diffHunks);
  const fragmentEndOffset = context.diffOriginalOffset + context.diffOriginalText.length;

  return {
    content:
      context.content.slice(0, context.diffOriginalOffset) +
      updatedFragment +
      context.content.slice(fragmentEndOffset),
    diffHunks: [],
    diffOriginalText: '',
    diffOriginalOffset: 0,
    isDiffMode: false,
    isDirty: true,
  };
}
