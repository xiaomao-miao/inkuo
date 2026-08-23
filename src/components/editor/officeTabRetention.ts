/**
 * Office editors keep their unsaved document model inside the editor engine.
 * An inactive clean tab can be reconstructed from disk/cache, but an inactive
 * dirty tab must remain mounted until it is saved or explicitly discarded.
 */
export function shouldMountOfficeTab(isActive: boolean, isDirty: boolean): boolean {
  return isActive || isDirty;
}
