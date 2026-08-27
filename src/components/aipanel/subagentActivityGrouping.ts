import type { SubagentActivity } from '../../types';

export interface DelegateCallRef {
  id: string;
  expert: string;
  task: string;
}

/**
 * Associate every nested sub-agent run with one concrete `delegate_to` call.
 * New stream events carry `parentToolCallId` and take the exact path. For old
 * persisted snapshots, pair unscoped runs to calls in task/execution order so
 * repeated use of the same expert never makes later cards inherit earlier
 * conversations.
 */
export function groupSubagentActivitiesByDelegate(
  delegateCalls: DelegateCallRef[],
  activities: SubagentActivity[] | undefined,
): Map<string, SubagentActivity[]> {
  const grouped = new Map<string, SubagentActivity[]>();
  for (const call of delegateCalls) grouped.set(call.id, []);
  if (!activities?.length) return grouped;

  const knownCallIds = new Set(delegateCalls.map((call) => call.id));
  const unscoped: SubagentActivity[] = [];
  for (const activity of activities) {
    if (activity.parentToolCallId && knownCallIds.has(activity.parentToolCallId)) {
      grouped.get(activity.parentToolCallId)?.push(activity);
    } else if (!activity.parentToolCallId) {
      unscoped.push(activity);
    }
  }

  for (const activity of unscoped) {
    const candidates = delegateCalls.filter((call) => (
      call.expert === activity.expert && (grouped.get(call.id)?.length ?? 0) === 0
    ));
    const owner = candidates.find((call) => call.task === activity.task) ?? candidates[0];
    if (owner) grouped.get(owner.id)?.push(activity);
  }

  return grouped;
}
