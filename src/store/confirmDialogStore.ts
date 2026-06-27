import { create } from 'zustand';

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  resolve: (confirmed: boolean) => void;
}

interface ConfirmDialogState {
  request: ConfirmRequest | null;
  ask: (req: Omit<ConfirmRequest, 'resolve'>) => Promise<boolean>;
  close: (result: boolean) => void;
}

/**
 * Promise-based confirmation dialog. Multiple in-flight requests would lose
 * earlier ones, so we keep one at a time and reject the new one if one is
 * already showing (callers should serialize).
 */
export const useConfirmDialogStore = create<ConfirmDialogState>((set, get) => ({
  request: null,
  ask: ({ title, message, confirmLabel, cancelLabel, danger }) =>
    new Promise<boolean>((resolve) => {
      if (get().request) {
        // Already showing; resolve immediately with false so the caller
        // doesn't hang waiting on a hidden dialog.
        resolve(false);
        return;
      }
      set({
        request: { title, message, confirmLabel, cancelLabel, danger, resolve },
      });
    }),
  close: (result) => {
    const current = get().request;
    if (!current) return;
    current.resolve(result);
    set({ request: null });
  },
}));
