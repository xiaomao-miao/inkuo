import { createContext } from 'react';
import type { CompletionItem } from '../../types/inline-complete';

export interface InlineCompleteContextValue {
  isEnabled: boolean;
  currentCompletion: CompletionItem | null;
  isLoading: boolean;
  error: string | null;
  triggerCompletion: (params: {
    document: string;
    cursorPosition: number;
    language: string;
    filePath?: string;
    snippet?: { text: string; start_offset: number };
  }) => Promise<void>;
  acceptCompletion: () => CompletionItem | null;
  dismissCompletion: () => void;
  setEnabled: (enabled: boolean) => void;
}

export const InlineCompleteContext = createContext<InlineCompleteContextValue | null>(null);
