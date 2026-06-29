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

  const oldLength = oldContent.length;
  // Prefer the hunk's declared offset, but fall back to the location where
  // `oldContent` was actually found. This guards against offset drift when
  // surrounding text has changed (e.g. after applying a previous hunk),
  // which would otherwise slice the wrong region and corrupt the result.
  const declaredOffsetMatch = originalText.indexOf(oldContent, hunk.old_offset) === hunk.old_offset;
  const matchOffset = declaredOffsetMatch ? hunk.old_offset : originalText.indexOf(oldContent, hunk.old_offset);

  if (matchOffset === -1) {
    return originalText.slice(0, hunk.old_offset) + newContent + originalText.slice(hunk.old_offset + oldLength);
  }

  return (
    originalText.slice(0, matchOffset) +
    newContent +
    originalText.slice(matchOffset + oldLength)
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
