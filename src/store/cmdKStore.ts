import { create } from 'zustand';

interface CmdKState {
  isOpen: boolean;
  scope: 'selection' | 'paragraph' | 'section' | 'document';
  instruction: string;
  isProcessing: boolean;

  open: () => void;
  close: () => void;
  setScope: (scope: 'selection' | 'paragraph' | 'section' | 'document') => void;
  setInstruction: (instruction: string) => void;
  setIsProcessing: (processing: boolean) => void;
  reset: () => void;
}

export const useCmdKStore = create<CmdKState>((set) => ({
  isOpen: false,
  scope: 'selection',
  instruction: '',
  isProcessing: false,

  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false, instruction: '', isProcessing: false }),
  setScope: (scope) => set({ scope }),
  setInstruction: (instruction) => set({ instruction }),
  setIsProcessing: (processing) => set({ isProcessing: processing }),
  reset: () => set({ scope: 'selection', instruction: '', isProcessing: false }),
}));
