// Small path helpers used by the workspace context menu when creating
// new files / folders and computing the "next unique name" strategy
// for entries being copied into a directory that may already have a
// name collision.
//
// These wrap `utils/path` (which handles OS-aware separators) but
// add the menu-specific behavior of stripping a trailing extension,
// appending a `-<timestamp>`, and re-attaching the extension.

import {
  getBaseName,
  getDirName,
  joinPath as joinDirPath,
} from '../../../utils/path';

/** Last path component, regardless of OS separator. */
export function basename(path: string): string {
  return getBaseName(path);
}

/** Directory portion of `path`, or empty string for root. */
export function parentPath(path: string): string {
  return getDirName(path);
}

/**
 * Join a parent dir and a child name. `parent` may be empty (root).
 */
export function joinPath(parent: string, name: string): string {
  return joinDirPath(parent, name);
}

/**
 * Pick the substring before the final `.`, or the whole input when
 * there is no extension. The result is the "stem" used to assemble
 * unique sibling names.
 */
export function fileStem(name: string): string {
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return name; // no extension, or hidden file like `.gitignore`
  return name.slice(0, dot);
}

/** Return the substring after the last `.`, or empty string. */
export function fileExtension(name: string): string {
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return '';
  return name.slice(dot);
}

/**
 * Compose a unique sibling name for `parentPath` that won't collide
 * with whatever already exists. Strategy: insert a `-<unix-ms>`
 * suffix before the extension. This is the same approach the
 * Sidebar's inline-create path uses when a user types a taken name
 * and presses Enter, so users see consistent auto-rename behavior
 * whether they're typing or pasting.
 */
export function uniqueSiblingName(parent: string, currentName: string): string {
  const stem = fileStem(currentName);
  const ext = fileExtension(currentName);
  return joinPath(parent, `${stem}-${Date.now()}${ext}`);
}
