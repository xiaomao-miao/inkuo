// Hook for handling Tab key press to trigger inline completion

import { useCallback, useEffect, useRef } from 'react';
import { useInlineComplete } from './useInlineComplete';
import { detectLanguage } from '../../types/inline-complete';

interface UseInlineCompleteOptions {
  /** Get the current document content */
  getDocument: () => string;
  /** Get the current cursor position (character offset) */
  getCursorPosition: () => number;
  /** Get the current file path */
  getFilePath?: () => string | undefined;
  /** Callback when completion is accepted */
  onAccept?: (text: string) => void;
  /** Callback when completion is dismissed */
  onDismiss?: () => void;
}

/**
 * Hook to handle inline completion trigger and acceptance
 *
 * Usage:
 * ```tsx
 * const { triggerCompletion } = useInlineCompleteTrigger({
 *   getDocument: () => editorContent,
 *   getCursorPosition: () => cursorOffset,
 *   getFilePath: () => currentFilePath,
 *   onAccept: (text) => insertText(text),
 * });
 * ```
 */
export function useInlineCompleteTrigger(options: UseInlineCompleteOptions) {
  const { isEnabled, currentCompletion, triggerCompletion } = useInlineComplete();

  const optionsRef = useRef(options);
  optionsRef.current = options;

  // Listen for accept events from the provider
  useEffect(() => {
    const handleAccept = (e: CustomEvent<{ text: string }>) => {
      const text = e.detail.text;
      optionsRef.current.onAccept?.(text);
    };

    window.addEventListener('inline-complete-accept', handleAccept as EventListener);
    return () => {
      window.removeEventListener('inline-complete-accept', handleAccept as EventListener);
    };
  }, []);

  // Handle Tab key press to trigger completion
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    // Only handle Tab key
    if (e.key !== 'Tab') return;

    // If there's a current completion, let the provider handle it
    if (currentCompletion) {
      return; // Provider handles Tab/Escape
    }

    // Trigger new completion on Tab press
    if (isEnabled) {
      e.preventDefault();

      const document = optionsRef.current.getDocument();
      const cursorPosition = optionsRef.current.getCursorPosition();
      const filePath = optionsRef.current.getFilePath?.();
      const language = detectLanguage(filePath);

      triggerCompletion({
        document,
        cursorPosition,
        language,
        filePath,
      });
    }
  }, [isEnabled, currentCompletion, triggerCompletion]);

  return {
    handleKeyDown,
    isEnabled,
    hasCompletion: !!currentCompletion,
  };
}

/**
 * Hook to insert completion text at cursor position
 *
 * This hook provides a function that can be used to insert text
 * at the current cursor position in CodeMirror.
 */
export function useInlineCompleteInsert() {
  const { acceptCompletion } = useInlineComplete();

  return {
    acceptAndInsert: acceptCompletion,
  };
}
