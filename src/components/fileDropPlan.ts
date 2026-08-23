import { getDirName, isPathInside, normalizeDirPath } from '../utils/path';

export interface DroppedPathInfo {
  path: string;
  isDirectory: boolean;
}

export interface FileDropPlan {
  workspaceToOpen: string | null;
  filesToOpen: string[];
  skippedPaths: string[];
}

/** Pure planning step kept separate from Tauri I/O for regression coverage. */
export function planFileDrop(
  entries: DroppedPathInfo[],
  currentWorkspace: string | null,
): FileDropPlan {
  const directory = entries.find((entry) => entry.isDirectory);
  const files = entries.filter((entry) => !entry.isDirectory).map((entry) => entry.path);
  if (directory) {
    const workspace = normalizeDirPath(directory.path);
    return {
      workspaceToOpen: workspace,
      filesToOpen: files.filter((path) => isPathInside(workspace, path)),
      skippedPaths: [
        ...entries.filter((entry) => entry.isDirectory && entry !== directory).map((entry) => entry.path),
        ...files.filter((path) => !isPathInside(workspace, path)),
      ],
    };
  }
  if (files.length === 0) {
    return { workspaceToOpen: null, filesToOpen: [], skippedPaths: [] };
  }
  const normalizedCurrent = currentWorkspace ? normalizeDirPath(currentWorkspace) : null;
  const firstExternalFile = normalizedCurrent
    ? files.find((path) => !isPathInside(normalizedCurrent, path))
    : files[0];
  const workspaceToOpen = firstExternalFile ? getDirName(firstExternalFile) : null;
  const effectiveWorkspace = workspaceToOpen ?? normalizedCurrent;
  const filesToOpen = effectiveWorkspace
    ? files.filter((path) => isPathInside(effectiveWorkspace, path))
    : [];
  return {
    workspaceToOpen,
    filesToOpen,
    skippedPaths: files.filter((path) => !filesToOpen.includes(path)),
  };
}
