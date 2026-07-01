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
  /// Hunk ids that could not be applied safely (the offset was stale and the
  /// fallback `indexOf` did not find `oldContent` in the original text). The
  /// caller can surface these in the UI so the user knows to re-trigger the
  /// diff instead of silently accepting a corrupted document.
  failedHunkIds: string[];
}

interface ApplyHunkResult {
  text: string;
  /** True if the hunk was applied. False if it was not safe to apply. */
  applied: boolean;
}

/// Build the start..end byte range in `originalText` where a particular
/// hunk is allowed to look for `oldContent`. When applying hunks in
/// descending offset order the region of `originalText` ahead of this hunk
/// (i.e. before `hunk.old_offset`) is not yet mutated, so the search must
/// stop at the next hunk's start to avoid matching an identical substring
/// that lives in an unrelated part of the document.
function searchWindowForHunk(originalText: string, hunk: DiffHunk, otherHunks: DiffHunk[]): [number, number] {
  let windowStart = 0;
  let windowEnd = originalText.length;
  for (const other of otherHunks) {
    if (other === hunk) continue;
    // Length of `other` in the original text (excludes pure inserts which
    // don't consume source bytes).
    const otherLength = other.changes
      .filter((c) => c.tag !== 'insert')
      .reduce((sum, c) => sum + c.content.length, 0);
    const otherEnd = other.old_offset + otherLength;

    if (other.old_offset > hunk.old_offset) {
      // `other` is later in the original text; cap the search end at its
      // start so a duplicate of `oldContent` between hunks doesn't get
      // matched first.
      windowEnd = Math.min(windowEnd, other.old_offset);
    } else if (otherEnd < hunk.old_offset) {
      // `other` is earlier in the original text; the search must start
      // after its tail (i.e. not be allowed to re-match text that
      // belongs to an earlier hunk).
      windowStart = Math.max(windowStart, otherEnd);
    }
  }
  return [windowStart, windowEnd];
}

function applyHunkToText(
  originalText: string,
  hunk: DiffHunk,
  otherHunks: DiffHunk[],
): ApplyHunkResult {
  const oldContent = hunk.changes
    .filter((change) => change.tag !== 'insert')
    .map((change) => change.content)
    .join('');
  const newContent = hunk.changes
    .filter((change) => change.tag !== 'delete')
    .map((change) => change.content)
    .join('');

  const oldLength = oldContent.length;
  if (oldLength === 0) {
    // Pure-insert hunk: just splice in `newContent` at the declared offset.
    // No search needed because there's nothing to match.
    const safeOffset = Math.max(0, Math.min(hunk.old_offset, originalText.length));
    return {
      text: originalText.slice(0, safeOffset) + newContent + originalText.slice(safeOffset),
      applied: true,
    };
  }

  // Restrict the search to the [windowStart, windowEnd) range. Without this
  // an `oldContent` substring that legitimately appears in two unrelated
  // places of the document (e.g. "fn main()" boilerplate) would be matched
  // in the wrong location by the fallback path.
  const [windowStart, windowEnd] = searchWindowForHunk(originalText, hunk, otherHunks);

  // 1) Try the declared offset exactly.
  if (
    hunk.old_offset >= windowStart
    && hunk.old_offset + oldLength <= windowEnd
    && originalText.startsWith(oldContent, hunk.old_offset)
  ) {
    return {
      text:
        originalText.slice(0, hunk.old_offset) +
        newContent +
        originalText.slice(hunk.old_offset + oldLength),
      applied: true,
    };
  }

  // 2) Fall back to a windowed `indexOf` search starting at the declared
  //    offset. We only accept a hit that lies inside the [windowStart,
  //    windowEnd) range so we never splice text that belongs to a
  //    different region of the document.
  let matchOffset = -1;
  const searchFrom = Math.max(windowStart, hunk.old_offset);
  while (searchFrom <= windowEnd - oldLength) {
    const found = originalText.indexOf(oldContent, searchFrom);
    if (found === -1 || found >= windowEnd) break;
    matchOffset = found;
    break;
  }

  if (matchOffset === -1) {
    // 3) Last-resort: do NOT slice with the unverified declared offset.
    //    Doing so was the source of a silent-corruption bug: the bytes
    //    at `hunk.old_offset` are not actually `oldContent`, so dropping
    //    `oldLength` bytes from there and replacing with `newContent`
    //    mutates the wrong region of the document. Returning the original
    //    text untouched and flagging the hunk as failed is the safe move
    //    — the caller can leave the hunk in the list and surface the
    //    failure to the user.
    return { text: originalText, applied: false };
  }

  return {
    text:
      originalText.slice(0, matchOffset) +
      newContent +
      originalText.slice(matchOffset + oldLength),
    applied: true,
  };
}

function applyAllHunksToText(originalText: string, hunks: DiffHunk[]): { text: string; failedHunkIds: string[] } {
  // Sort descending by `old_offset` so applying a later hunk first does
  // not invalidate the byte offsets of earlier hunks.
  const sortedHunks = [...hunks].sort((left, right) => right.old_offset - left.old_offset);
  const failedHunkIds: string[] = [];
  const text = sortedHunks.reduce((acc, hunk) => {
    const result = applyHunkToText(acc, hunk, sortedHunks);
    if (!result.applied) {
      failedHunkIds.push(hunk.id);
    }
    return result.text;
  }, originalText);
  return { text, failedHunkIds };
}

export function applySelectedHunk(context: DiffContext, hunkId: string): DiffResolution | null {
  const hunk = context.diffHunks.find((candidate) => candidate.id === hunkId);
  if (!hunk) {
    return null;
  }

  const otherHunks = context.diffHunks.filter((candidate) => candidate.id !== hunkId);
  const { text: updatedFragment, applied } = applyHunkToText(
    context.diffOriginalText,
    hunk,
    [...otherHunks, hunk],
  );
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
    failedHunkIds: applied ? [] : [hunkId],
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
    failedHunkIds: [],
  };
}

export function applyRemainingHunks(context: DiffContext): DiffResolution {
  const { text: updatedFragment, failedHunkIds } = applyAllHunksToText(
    context.diffOriginalText,
    context.diffHunks,
  );
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
    failedHunkIds,
  };
}
