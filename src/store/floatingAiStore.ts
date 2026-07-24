// Multi-instance floating AI popovers — one per ask.
//
// The docx right-click menu spawns these popovers on demand for
// "用 AI 解释 / 翻译 / 总结 / 改写" prompts. Each popover is
// independent: it has its own session id, stream subscription, and
// viewport position. The store here only owns the metadata; the
// actual stream subscription lives in `useFloatingAiStream`.
//
// Why a separate store from `useAIPanelStore`?
//   - The AI panel's sessions are persistent: a user can scroll back
//     to last week's chat. Popovers are ephemeral — they exist only
//     while the user has them open, and closing them discards the
//     transcript.
//   - Popovers stream directly through the agent's `ask` mode
//     (read-only) without touching the panel's tool registry, baseline
//     rollback, or plan file machinery.
//   - Keeping them isolated also means we don't pollute the panel's
//     `sessions` array with single-shot asks the user is unlikely to
//     scroll back to.

import { create } from 'zustand';

export type FloatingAiStatus = 'idle' | 'streaming' | 'done' | 'error' | 'cancelled';

export interface FloatingAiWindow {
  /** Stable id used as the Tauri session id. */
  id: string;
  /** Short title shown in the window header (e.g. "AI 解释"). */
  title: string;
  /** Optional sub-label (e.g. "选区 · 124 字"). */
  subtitle?: string;
  /** The original quoted text — shown above the stream so the user
   *  remembers what they asked about. */
  quote: string;
  /** The actual instruction sent to the model. Distinct from
   *  `quote` because most prompts wrap the quote in a template
   *  (e.g. "请解释以下内容：\n\n"""...""""). The window component
   *  reads `quote` for display and `instruction` for the stream
   *  input. */
  instruction: string;
  /** Initial position in viewport coordinates. The store is updated
   *  on drag, so a fresh `open` should pick a position that doesn't
   *  collide with existing windows (caller's responsibility). */
  position: { x: number; y: number };
  /** Window width in px. Defaults to 480 if not provided. */
  width?: number;
  /** Window height in px. Defaults to 440 (double the original
   *  minimum) so the body has room for a meaningful AI response on
   *  first open. */
  height?: number;
  /** Stream status. */
  status: FloatingAiStatus;
  /** Accumulated text from `text` events. */
  streamedContent: string;
  /** Error message when status === 'error'. */
  errorMessage?: string;
  /** Timestamp when opened — used to order z-index. Later = on top. */
  openedAt: number;
}

interface FloatingAiState {
  windows: Record<string, FloatingAiWindow>;
  /** Render order, most-recently-opened last. */
  order: string[];
  open: (input: {
    id?: string;
    title: string;
    subtitle?: string;
    quote: string;
    instruction: string;
    position: { x: number; y: number };
    width?: number;
    height?: number;
  }) => string;
  close: (id: string) => void;
  bringToFront: (id: string) => void;
  setPosition: (id: string, position: { x: number; y: number }) => void;
  /** Resize the popover. Both dimensions are clamped to reasonable
   *  minima so a tiny resize still keeps the popover usable. */
  setSize: (id: string, size: { width: number; height: number }) => void;
  setStatus: (id: string, status: FloatingAiStatus, errorMessage?: string) => void;
  appendDelta: (id: string, delta: string) => void;
  finish: (id: string, content: string) => void;
}

/**
 * Stable id helper. We re-use the supplied `id` if given so the
 * caller (e.g. a context-menu action) can tie the popover back to the
 * request that spawned it. Without an id we mint a `crypto.randomUUID`
 * — guaranteed collision-free in the browser.
 */
const newId = (): string =>
  (typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `fai-${Date.now()}-${Math.random().toString(36).slice(2)}`);

export const useFloatingAiStore = create<FloatingAiState>((set) => ({
  windows: {},
  order: [],
  open: (input) => {
    const id = input.id ?? newId();
    set((state) => {
      // If a window with this id already exists, bring it to the
      // front instead of replacing it (the caller may want to
      // re-raise a stale popover). For now the simpler "replace"
      // semantics is fine — popovers are ephemeral.
      const next: FloatingAiWindow = {
        id,
        title: input.title,
        subtitle: input.subtitle,
        quote: input.quote,
        instruction: input.instruction,
        position: input.position,
        width: input.width,
        height: input.height,
        status: 'idle',
        streamedContent: '',
        openedAt: Date.now(),
      };
      return {
        windows: { ...state.windows, [id]: next },
        order: [...state.order.filter((x) => x !== id), id],
      };
    });
    return id;
  },
  close: (id) =>
    set((state) => {
      const { [id]: _drop, ...rest } = state.windows;
      return {
        windows: rest,
        order: state.order.filter((x) => x !== id),
      };
    }),
  bringToFront: (id) =>
    set((state) =>
      state.windows[id]
        ? { order: [...state.order.filter((x) => x !== id), id] }
        : state,
    ),
  setPosition: (id, position) =>
    set((state) => {
      const w = state.windows[id];
      if (!w) return state;
      return {
        windows: { ...state.windows, [id]: { ...w, position } },
      };
    }),
  setSize: (id, size) =>
    set((state) => {
      const w = state.windows[id];
      if (!w) return state;
      // Floor dimensions at 240×180 so the popover always stays
      // usable (header + footer + at least one line of content).
      const width = Math.max(240, Math.round(size.width));
      const height = Math.max(180, Math.round(size.height));
      return {
        windows: { ...state.windows, [id]: { ...w, width, height } },
      };
    }),
  setStatus: (id, status, errorMessage) =>
    set((state) => {
      const w = state.windows[id];
      if (!w) return state;
      return {
        windows: {
          ...state.windows,
          [id]: { ...w, status, errorMessage: errorMessage ?? w.errorMessage },
        },
      };
    }),
  appendDelta: (id, delta) =>
    set((state) => {
      const w = state.windows[id];
      if (!w || !delta) return state;
      return {
        windows: {
          ...state.windows,
          [id]: { ...w, streamedContent: w.streamedContent + delta },
        },
      };
    }),
  finish: (id, content) =>
    set((state) => {
      const w = state.windows[id];
      if (!w) return state;
      return {
        windows: {
          ...state.windows,
          [id]: {
            ...w,
            status: 'done',
            streamedContent: content || w.streamedContent,
          },
        },
      };
    }),
}));
