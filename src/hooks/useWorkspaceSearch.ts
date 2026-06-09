import { useMemo, useState } from 'react';
import type { FileEntry } from '../types';

function getEntriesWithParents(matchingEntries: FileEntry[], allEntries: FileEntry[]): FileEntry[] {
  const resultSet = new Set<string>();

  for (const entry of matchingEntries) {
    resultSet.add(entry.path);

    const parts = entry.path.split('/');
    for (let index = 1; index < parts.length; index += 1) {
      resultSet.add(parts.slice(0, index).join('/'));
    }
  }

  return allEntries.filter((entry) => resultSet.has(entry.path));
}

function sortEntries(entries: FileEntry[]): FileEntry[] {
  return [...entries].sort((left, right) => {
    if (left.is_dir && !right.is_dir) return -1;
    if (!left.is_dir && right.is_dir) return 1;
    return left.name.localeCompare(right.name);
  });
}

export function useWorkspaceSearch(files: FileEntry[], workspacePath: string | null) {
  const [searchQuery, setSearchQuery] = useState('');

  const filteredFiles = useMemo(() => {
    const rootEntries = files.filter((entry) => {
      if (!workspacePath) return true;
      const relativePath = entry.path.replace(`${workspacePath}/`, '');
      return !relativePath.includes('/');
    });

    if (!searchQuery) {
      return sortEntries(rootEntries);
    }

    const matchingEntries = files.filter((entry) => entry.name.toLowerCase().includes(searchQuery.toLowerCase()));
    return sortEntries(getEntriesWithParents(matchingEntries, files));
  }, [files, searchQuery, workspacePath]);

  return {
    searchQuery,
    setSearchQuery,
    filteredFiles,
  };
}
