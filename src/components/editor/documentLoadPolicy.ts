/**
 * Disk data may replace the editor only when there is no recoverable local
 * buffer, the buffer is clean, or the user explicitly chose to discard it.
 */
export function shouldApplyDiskDocument(
  hasLocalBuffer: boolean,
  liveTabIsDirty: boolean,
  userApprovedDiscard: boolean,
): boolean {
  return !hasLocalBuffer || !liveTabIsDirty || userApprovedDiscard;
}
