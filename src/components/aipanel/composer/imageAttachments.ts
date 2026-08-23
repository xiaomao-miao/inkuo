import type { ImageAttachmentInput } from '../../../types';

export const MAX_COMPOSER_IMAGE_ATTACHMENTS = 8;

const SUPPORTED_IMAGE_EXTENSION = /\.(png|jpe?g|webp|gif)$/i;

/** Convert native file-dialog paths into the provider-neutral attachment
 * contract while de-duplicating and enforcing the backend's request count
 * ceiling. Byte/MIME/workspace validation remains authoritative in Rust. */
export function appendImagePaths(
  current: ImageAttachmentInput[],
  selectedPaths: string[],
): ImageAttachmentInput[] {
  const next = current.map((attachment) => ({ ...attachment }));
  const seen = new Set(
    next.flatMap((attachment) => attachment.path ? [attachment.path] : []),
  );

  for (const rawPath of selectedPaths) {
    const path = rawPath.trim();
    if (!path || seen.has(path) || !SUPPORTED_IMAGE_EXTENSION.test(path)) continue;
    seen.add(path);
    next.push({
      path,
      detail: 'high',
      name: path.split(/[\\/]/).pop() || 'image',
    });
    if (next.length >= MAX_COMPOSER_IMAGE_ATTACHMENTS) break;
  }
  return next;
}
