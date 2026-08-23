export type ExternalRefreshDecision = 'reload' | 'show-conflict' | 'ignore';

/**
 * Decide what an editor should do when its backing file changes on disk.
 *
 * Keeping this policy separate from React makes the data-loss invariant easy
 * to test: a dirty editor is never eligible for an automatic reload. The
 * `explicitReloadInProgress` branch coalesces duplicate watcher/stream events
 * while the user-selected disk version is already being loaded.
 */
export function decideExternalRefresh(
  isDirty: boolean,
  explicitReloadInProgress: boolean,
): ExternalRefreshDecision {
  if (explicitReloadInProgress) return 'ignore';
  return isDirty ? 'show-conflict' : 'reload';
}
