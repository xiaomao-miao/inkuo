import { useState, useCallback, useEffect, useRef } from 'react';
import type { FileEntry } from '../types';
import { searchFiles } from '../services/workspace';

interface UseWorkspaceSearchResult {
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  searchResults: FileEntry[];
  isSearching: boolean;
  searchError: string | null;
  clearSearch: () => void;
}

export function useWorkspaceSearch(workspacePath: string | null): UseWorkspaceSearchResult {
  const [searchQuery, setSearchQueryState] = useState('');
  const [searchResults, setSearchResults] = useState<FileEntry[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);

  const searchTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const abortControllerRef = useRef<AbortController | null>(null);

  const clearSearch = useCallback(() => {
    abortControllerRef.current?.abort();
    abortControllerRef.current = null;
    if (searchTimeoutRef.current) clearTimeout(searchTimeoutRef.current);
    searchTimeoutRef.current = null;
    setSearchQueryState('');
    setSearchResults([]);
    setIsSearching(false);
    setSearchError(null);
  }, []);

  const setSearchQuery = useCallback(
    (query: string) => {
      setSearchQueryState(query);
      setSearchError(null);

      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }

      if (searchTimeoutRef.current) {
        clearTimeout(searchTimeoutRef.current);
      }

      if (!query.trim()) {
        setSearchResults([]);
        setIsSearching(false);
        return;
      }

      if (!workspacePath) {
        return;
      }

      setIsSearching(true);

      searchTimeoutRef.current = setTimeout(async () => {
        const controller = new AbortController();
        abortControllerRef.current = controller;

        try {
          const results = await searchFiles(workspacePath, query.trim());
          if (!controller.signal.aborted) {
            setSearchResults(results);
          }
        } catch {
          if (!controller.signal.aborted) {
            setSearchResults([]);
            setSearchError('搜索失败，请重试');
          }
        } finally {
          if (!controller.signal.aborted) {
            setIsSearching(false);
          }
        }
      }, 200);
    },
    [workspacePath],
  );

  useEffect(() => {
    return () => {
      if (searchTimeoutRef.current) {
        clearTimeout(searchTimeoutRef.current);
      }
      if (abortControllerRef.current) {
        abortControllerRef.current.abort();
      }
    };
  }, []);

  // Search results belong to one workspace. Clear them immediately when the
  // user switches roots so an old path can never be opened from the new tree.
  useEffect(() => {
    clearSearch();
  }, [workspacePath, clearSearch]);

  return {
    searchQuery,
    setSearchQuery,
    searchResults,
    isSearching,
    searchError,
    clearSearch,
  };
}
