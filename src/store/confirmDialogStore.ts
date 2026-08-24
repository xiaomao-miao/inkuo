import { create } from 'zustand';

export interface ConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  secondaryLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  resolve: (result: ConfirmDialogResult) => void;
}

export type ConfirmDialogResult = 'confirm' | 'secondary' | 'cancel';
type ConfirmRequestOptions = Omit<ConfirmRequest, 'resolve'>;

interface ConfirmDialogState {
  request: ConfirmRequest | null;
  ask: (req: ConfirmRequestOptions) => Promise<boolean>;
  askChoice: (req: ConfirmRequestOptions) => Promise<ConfirmDialogResult>;
  close: (result: ConfirmDialogResult | boolean) => void;
}

/**
 * Promise-based confirmation dialog. Multiple in-flight requests would lose
 * earlier ones, so we keep one at a time and reject the new one if one is
 * already showing (callers should serialize).
 */
export const useConfirmDialogStore = create<ConfirmDialogState>((set, get) => {
  const askChoice = (req: ConfirmRequestOptions) =>
    new Promise<ConfirmDialogResult>((resolve) => {
      if (get().request) {
        // Already showing; resolve immediately with cancel so the caller
        // doesn't hang waiting on a hidden dialog.
        resolve('cancel');
        return;
      }
      set({ request: { ...req, resolve } });
    });

  return {
    request: null,
    askChoice,
    ask: async (req) => (await askChoice(req)) === 'confirm',
    close: (result) => {
      const current = get().request;
      if (!current) return;
      const normalized = typeof result === 'boolean'
        ? result ? 'confirm' : 'cancel'
        : result;
      current.resolve(normalized);
      set({ request: null });
    },
  };
});
