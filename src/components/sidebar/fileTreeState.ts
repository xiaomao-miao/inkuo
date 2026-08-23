export type DirectoryContentState = 'loading' | 'error' | 'empty' | 'populated';

/**
 * Resolve directory body state independently of a background refresh.
 * A cached empty array is real data, so stale-while-revalidate must keep the
 * empty state stable instead of alternating it with a loading placeholder.
 */
export function getDirectoryContentState(
  isKnown: boolean,
  childCount: number,
  isLoading = !isKnown,
  hasError = false,
): DirectoryContentState {
  // Cached data wins over a failed background refresh. The caller can render
  // a non-destructive retry notice alongside the stale entries, while the
  // directory body itself remains stable.
  if (!isKnown) {
    if (isLoading) return 'loading';
    if (hasError) return 'error';
    return 'loading';
  }
  return childCount === 0 ? 'empty' : 'populated';
}
