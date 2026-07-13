/**
 * Cross-platform path helpers used by the workspace file tree.
 *
 * The Rust backend returns paths in the platform-native form (`\` on Windows,
 * `/` on macOS / Linux). Frontend code, however, needs to compare, split, and
 * reconstruct these paths without crashing on either separator.
 *
 * The convention used here:
 *   - `normalizeDirPath` collapses all separators to `/` and strips trailing
 *     slashes, so internal data structures (cache keys, expanded dir sets,
 *     relative-path comparisons) are separator-agnostic.
 *   - When we hand a path back to the backend, we send the original
 *     (separator-native) form because that is what `std::fs::read_dir`
 *     expects. Internal storage is the normalized form.
 *
 * Keep this module dependency-free so it can be imported from any layer.
 */

const SEPARATOR_REGEX = /[\\/]+/g;

/**
 * Collapse all `\` and `/` separators to `/` and trim trailing separators.
 * Returns the empty string for falsy input.
 *
 *   normalizeDirPath('E:\\文档\\sub') === 'E:/文档/sub'
 *   normalizeDirPath('/foo/bar/')    === '/foo/bar'
 *   normalizeDirPath('')             === ''
 */
export function normalizeDirPath(path: string): string {
  if (!path) return '';
  return path.replace(SEPARATOR_REGEX, '/').replace(/\/+$/, '');
}

/**
 * Returns true when `child` is `parent` itself or sits beneath it. Both
 * arguments are normalized before comparison, so callers can pass either
 * separator style.
 *
 *   isPathInside('E:\\文档', 'E:\\文档')              === true
 *   isPathInside('E:\\文档\\sub\\file.md', 'E:\\文档') === true
 *   isPathInside('E:\\文档2', 'E:\\文档')              === false
 */
export function isPathInside(parent: string, child: string): boolean {
  const normalizedParent = normalizeDirPath(parent);
  const normalizedChild = normalizeDirPath(child);
  if (!normalizedParent || !normalizedChild) return false;
  if (normalizedChild === normalizedParent) return true;
  return normalizedChild.startsWith(`${normalizedParent}/`);
}

/**
 * Strip `parent` (and an optional leading separator) from the front of
 * `child`. Both inputs are normalized first.
 *
 *   getRelativePath('E:\\文档', 'E:\\文档\\a\\b.md') === 'a/b.md'
 */
export function getRelativePath(parent: string, child: string): string {
  const normalizedParent = normalizeDirPath(parent);
  const normalizedChild = normalizeDirPath(child);
  if (!normalizedParent) return normalizedChild;
  if (normalizedChild === normalizedParent) return '';
  if (normalizedChild.startsWith(`${normalizedParent}/`)) {
    return normalizedChild.slice(normalizedParent.length + 1);
  }
  return normalizedChild;
}

/**
 * Return the deepest path component as a basename. Splits on either
 * separator so it works for both `C:\\foo\\bar.md` and `/foo/bar.md`.
 */
export function getBaseName(path: string): string {
  if (!path) return '';
  const normalized = normalizeDirPath(path);
  if (!normalized) return '';
  const lastSlash = normalized.lastIndexOf('/');
  return lastSlash === -1 ? normalized : normalized.slice(lastSlash + 1);
}

/**
 * Return the parent directory of `path`, or an empty string if `path` has
 * no parent (e.g. `''` or a bare drive root).
 */
export function getDirName(path: string): string {
  const normalized = normalizeDirPath(path);
  if (!normalized) return '';
  const lastSlash = normalized.lastIndexOf('/');
  return lastSlash === -1 ? '' : normalized.slice(0, lastSlash);
}

/**
 * Join a parent directory with one or more segments using `/` separators.
 * The result is normalized so callers can use it as a cache key directly.
 *
 *   joinPath('E:\\文档', 'sub', 'nested') === 'E:/文档/sub/nested'
 */
export function joinPath(parent: string, ...segments: Array<string | null | undefined>): string {
  const parts = [parent, ...segments]
    .filter((segment): segment is string => typeof segment === 'string' && segment.length > 0);
  return normalizeDirPath(parts.join('/'));
}