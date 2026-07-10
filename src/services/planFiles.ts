import { invoke } from '@tauri-apps/api/core';

export interface PlanFileSaveResult {
  path: string;
  plan_id: string;
}

/**
 * Persist a plan (Markdown prose + JSON fence) as
 * `<workspace>/.inkuo/plans/<plan_id>.md`. Returns the absolute path and
 * the sanitized plan id we actually used as the filename stem.
 *
 * Every plan gets a persistent file the user can grep / open / share, and
 * the apply / cancel hooks know exactly which file to delete.
 */
export async function savePlanToFile(
  workspacePath: string,
  planId: string,
  content: string,
): Promise<PlanFileSaveResult> {
  return invoke<PlanFileSaveResult>('plan_save', {
    workspacePath,
    planId,
    content,
  });
}

/** Remove a plan's persisted md from disk. No-op if the file is missing. */
export async function deletePlanFile(
  workspacePath: string,
  planId: string,
): Promise<boolean> {
  return invoke<boolean>('plan_delete', { workspacePath, planId });
}

/**
 * Generate a stable, time-sortable plan id. Used as the filename stem
 * under `plans/` and as a unique key for apply / cancel flows.
 *
 * Format: `plan-YYYYMMDD-HHmmss-<6-char-base36>`. Timestamp component
 * keeps the directory ordered; base36 suffix avoids collisions when two
 * plans land in the same second.
 */
export function generatePlanId(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  const stamp =
    now.getFullYear().toString() +
    pad(now.getMonth() + 1) +
    pad(now.getDate()) +
    '-' +
    pad(now.getHours()) +
    pad(now.getMinutes()) +
    pad(now.getSeconds());
  const suffix = Math.floor(Math.random() * 36 ** 6)
    .toString(36)
    .padStart(6, '0');
  return `plan-${stamp}-${suffix}`;
}
