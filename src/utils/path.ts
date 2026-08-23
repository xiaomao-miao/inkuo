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
 * Drive-letter and UNC paths use Windows' case-insensitive comparison
 * semantics. Callers pass normalized paths so this helper does not need to
 * allocate another normalized copy.
 */
function usesWindowsPathSemantics(...paths: string[]): boolean {
  return paths.some((path) => /^[A-Za-z]:\//.test(path) || path.startsWith('//'));
}

/**
 * Collapse all `\` and `/` separators to `/` and trim trailing separators,
 * while preserving POSIX, drive, and UNC roots.
 * Returns the empty string for falsy input.
 *
 *   normalizeDirPath('E:\\文档\\sub') === 'E:/文档/sub'
 *   normalizeDirPath('/foo/bar/')    === '/foo/bar'
 *   normalizeDirPath('')             === ''
 */
export function normalizeDirPath(path: string): string {
  if (!path) return '';
  const isUnc = /^[\\/]{2}/.test(path);
  let normalized = path.replace(SEPARATOR_REGEX, '/');
  if (isUnc) normalized = `//${normalized.replace(/^\/+/, '')}`;
  if (normalized === '/' || normalized === '//' || /^[A-Za-z]:\/$/.test(normalized)) {
    return normalized;
  }
  return normalized.replace(/\/+$/, '');
}

/**
 * Whether `path` is absolute on one of the desktop platforms supported by
 * Tauri. `URL`-style paths are intentionally excluded: file events and drop
 * events give us native filesystem paths, never `file://` URLs.
 */
export function isAbsoluteFilePath(path: string): boolean {
  const normalized = normalizeDirPath(path);
  return (
    normalized.startsWith('/') ||
    normalized.startsWith('//') ||
    /^[A-Za-z]:\//.test(normalized)
  );
}

/**
 * Resolve a path emitted by an AI tool against the active workspace.
 *
 * Tool executors accept absolute paths, but older prompts and a few delegated
 * experts can still return a workspace-relative `file_path`. Tabs, meanwhile,
 * always hold the absolute path returned by `list_directory`. Comparing the
 * two strings directly is therefore unreliable and was the reason an Office
 * file could be updated on disk while its open editor kept showing old bytes.
 */
export function resolveWorkspaceFilePath(
  path: string,
  workspaceRoot: string | null | undefined,
): string {
  const normalized = normalizeDirPath(path);
  if (!normalized || isAbsoluteFilePath(normalized) || !workspaceRoot) {
    return normalized;
  }
  return joinPath(workspaceRoot, normalized);
}

/**
 * Cross-platform equality for filesystem paths. Windows drive and UNC paths
 * are case-insensitive; POSIX paths retain their case-sensitive semantics.
 */
export function areFilePathsEqual(left: string, right: string): boolean {
  const a = normalizeDirPath(left);
  const b = normalizeDirPath(right);
  const windowsLike = usesWindowsPathSemantics(a, b);
  return windowsLike ? a.toLowerCase() === b.toLowerCase() : a === b;
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
  if (areFilePathsEqual(normalizedChild, normalizedParent)) return true;
  const windowsLike = usesWindowsPathSemantics(normalizedParent, normalizedChild);
  const comparableParent = windowsLike ? normalizedParent.toLowerCase() : normalizedParent;
  const comparableChild = windowsLike ? normalizedChild.toLowerCase() : normalizedChild;
  const descendantPrefix = comparableParent.endsWith('/')
    ? comparableParent
    : `${comparableParent}/`;
  return comparableChild.startsWith(descendantPrefix);
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
  const windowsLike = usesWindowsPathSemantics(normalizedParent, normalizedChild);
  const comparableParent = windowsLike ? normalizedParent.toLowerCase() : normalizedParent;
  const comparableChild = windowsLike ? normalizedChild.toLowerCase() : normalizedChild;
  if (comparableChild === comparableParent) return '';
  const comparablePrefix = comparableParent.endsWith('/')
    ? comparableParent
    : `${comparableParent}/`;
  if (comparableChild.startsWith(comparablePrefix)) {
    // Slice the normalized original rather than the lower-cased comparison
    // string so the relative result retains the filesystem's display casing.
    return normalizedChild.slice(comparablePrefix.length);
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
  if (normalized === '/' || normalized === '//' || /^[A-Za-z]:\/$/.test(normalized)) return '';
  const lastSlash = normalized.lastIndexOf('/');
  if (lastSlash === 0) return '/';
  if (lastSlash === 2 && /^[A-Za-z]:\//.test(normalized)) return normalized.slice(0, 3);
  return lastSlash === -1 ? '' : normalized.slice(0, lastSlash);
}

/**
 * Return the directory that contains `filePath`, rooted at `workspaceRoot`.
 * Both inputs are normalised first so the math is separator-agnostic.
 *
 * Used by the file watcher: a `Created` / `Deleted` / `Modified` event
 * arrives with the changed file's absolute path; we need to know which
 * directory cache entry to invalidate.
 *
 *   getParentDirPath('/root', '/root/a.md')           === '/root'
 *   getParentDirPath('/root', '/root/sub/b.md')      === '/root/sub'
 *   getParentDirPath('/root', '/root/sub/nested/c')  === '/root/sub/nested'
 *   getParentDirPath('/root', '/root')               === null
 */
export function getParentDirPath(filePath: string, workspaceRoot: string): string | null {
  const normalizedRoot = normalizeDirPath(workspaceRoot);
  if (!normalizedRoot) return null;

  const relativePath = getRelativePath(normalizedRoot, filePath);
  if (!relativePath) return normalizedRoot;

  const segments = relativePath.split('/').filter(Boolean);
  if (segments.length <= 1) return normalizedRoot;

  return joinPath(normalizedRoot, ...segments.slice(0, -1));
}

/**
 * Join a parent directory with one or more segments using `/` separators.
 * The result is normalized so callers can use it as a cache key directly.
 *
 *   joinPath('E:\\文档', 'sub', 'nested') === 'E:/文档/sub/nested'
 */
export function joinPath(parent: string, ...segments: Array<string | null | undefined>): string {
  let result = normalizeDirPath(parent);
  for (const segment of segments) {
    if (!segment) continue;
    const cleanSegment = normalizeDirPath(segment).replace(/^\/+/, '');
    if (!cleanSegment) continue;
    if (!result) result = cleanSegment;
    else result = `${result}${result.endsWith('/') ? '' : '/'}${cleanSegment}`;
  }
  return normalizeDirPath(result);
}
