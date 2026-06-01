import { useEffect, useCallback } from 'react';

export interface UseKeyboardSaveOptions {
  onSave: () => void;
  enabled?: boolean;
}

/**
 * Global keyboard handler for Ctrl+S / Cmd+S save shortcut.
 * Registers on mount, unregisters on unmount.
 */
export function useKeyboardSave({ onSave, enabled = true }: UseKeyboardSaveOptions) {
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (!enabled) return;
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      e.preventDefault();
      onSave();
    }
  }, [onSave, enabled]);

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);
}
