/**
 * Floating quick-action toolbar that pops up when the user finishes
 * selecting text inside the AI panel's chat view.
 *
 * Inspired by the GitHub Copilot / Cursor / Notion AI pattern: the
 * selected span is the user's "intent handle" — they don't want to
 * type a free-form prompt, they want to point at something and say
 * "explain this", "expand this", "drop this".
 *
 * The toolbar shows four presets:
 *   1. 介绍      — summarise the selection
 *   2. 解释      — explain the selection
 *   3. 详细展开  — elaborate in more detail
 *   4. 拒绝      — feedback-style reply ("this answer is unsatisfying")
 *
 * Clicking a preset composes a prompt and dispatches it through
 * `sendWithPrompt`. We deliberately do NOT write the prompt into the
 * composer's `input` field: the user might still be typing their own
 * message, and displacing it would be surprising.
 *
 * The toolbar is positioned using the selection's bounding rect: above
 * the selection if there's room, otherwise below. It auto-dismisses
 * on scroll, on outside-click, on Escape, and when the selection is
 * collapsed or moved outside the chat container.
 */
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { BookOpen, HelpCircle, Maximize2, ThumbsDown, X } from 'lucide-react';
import styles from './SelectionQuickActions.module.css';

interface SelectionQuickActionsProps {
  /** The scroll container the selection lives in. The toolbar stays inside this
   *  element's coordinate space and is dismissed when the user scrolls it. */
  scrollContainer: HTMLElement | null;
  /** Dispatch the composed prompt. Bypasses the composer input so the user's
   *  in-progress text isn't overwritten. */
  onSend: (prompt: string) => Promise<void> | void;
  /** Disables the toolbar (e.g. while the AI is streaming). */
  disabled?: boolean;
}

type PresetId = 'intro' | 'explain' | 'expand' | 'reject';

/**
 * The four preset payloads. Each one wraps the selected text in a
 * triple-quoted block so the model has unambiguous delimiters and
 * doesn't conflate the prose with the user's instructions. The
 * "reject" preset is a feedback-style message: it tells the model the
 * selection wasn't what the user wanted, so the next reply can course
 * correct.
 */
const PRESETS: ReadonlyArray<{
  id: PresetId;
  label: string;
  icon: React.ReactNode;
  buildPrompt: (quote: string) => string;
}> = [
  {
    id: 'intro',
    label: '介绍',
    icon: <BookOpen size={12} />,
    buildPrompt: (q) => `请简要介绍以下内容：\n\n"""\n${q}\n"""`,
  },
  {
    id: 'explain',
    label: '解释',
    icon: <HelpCircle size={12} />,
    buildPrompt: (q) => `请帮我解释这段内容：\n\n"""\n${q}\n"""`,
  },
  {
    id: 'expand',
    label: '详细展开',
    icon: <Maximize2 size={12} />,
    buildPrompt: (q) => `请对以下内容进行更详细的展开说明：\n\n"""\n${q}\n"""`,
  },
  {
    id: 'reject',
    label: '拒绝',
    icon: <ThumbsDown size={12} />,
    buildPrompt: (q) => `我对这段回答/引用不满意，请忽略它并重新回答：\n\n"""\n${q}\n"""`,
  },
];

interface ToolbarState {
  selectedText: string;
  /** Selection rect in viewport coordinates. */
  rect: DOMRect;
}

/**
 * Minimum length of selected text before we show the toolbar. Avoiding
 * the toolbar for single-character selections keeps the UI from
 * flickering every time the user clicks-and-drags through punctuation.
 */
const MIN_SELECTION_LENGTH = 2;

export const SelectionQuickActions: React.FC<SelectionQuickActionsProps> = ({
  scrollContainer,
  onSend,
  disabled = false,
}) => {
  const [state, setState] = useState<ToolbarState | null>(null);
  /**
   * Which preset is the user currently keyboard-focusing. We keep the
   * current selected text in a ref too so the keyboard handler can
   * resolve a preset id without re-reading the live selection (which
   * may have collapsed by the time the user presses Enter).
   */
  const [focusedIndex, setFocusedIndex] = useState(0);
  const stateRef = useRef<ToolbarState | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);

  // Keep a ref in sync with the latest state so the keyboard handler
  // (registered once via useEffect) can read the current selection
  // without being re-bound on every render.
  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  /**
   * Read the current document selection, decide whether it's inside
   * the chat container, and if so update the toolbar state. Called on
   * every `selectionchange` (cheap: just two `getBoundingClientRect`
   * and a string trim).
   *
   * The actual `setState` is deferred to the next animation frame so
   * that the browser can paint the selection highlight before we
   * mount any DOM that might disrupt it. Without this defer, mounting
   * the toolbar in the same frame as the selection release would
   * occasionally cause Chrome to collapse the selection back to a
   * single character (the selection render and the toolbar mount
   * race, and the focus change inside `useEffect` is what the user
   * perceives as "the selection dropped to one char").
   */
  const recompute = useCallback(() => {
    if (disabled) return;
    const rafId = requestAnimationFrame(() => {
      if (disabled) return;
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
        setState(null);
        return;
      }
      const text = sel.toString().trim();
      if (text.length < MIN_SELECTION_LENGTH) {
        setState(null);
        return;
      }
      // Reject selections that originate outside the chat container —
      // e.g. the user actually highlighted something in the file
      // editor, not in the AI panel.
      const range = sel.getRangeAt(0);
      if (!scrollContainer) {
        setState(null);
        return;
      }
      if (!scrollContainer.contains(range.commonAncestorContainer)) {
        setState(null);
        return;
      }
      // Reject selections inside the composer's textarea — let the
      // browser's native text-edit menu handle those (the user is
      // editing their own draft, not asking about model output).
      const startNode = range.startContainer;
      const startEl =
        startNode.nodeType === Node.ELEMENT_NODE
          ? (startNode as Element)
          : startNode.parentElement;
      if (startEl?.closest('textarea, input, [contenteditable="true"]')) {
        setState(null);
        return;
      }
      const rect = range.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) {
        setState(null);
        return;
      }
      setState({ selectedText: text, rect });
    });
    return () => cancelAnimationFrame(rafId);
  }, [scrollContainer, disabled]);

  useEffect(() => {
    document.addEventListener('selectionchange', recompute);
    return () => document.removeEventListener('selectionchange', recompute);
  }, [recompute]);

  /**
   * Re-run the selection check on mouseup, because `selectionchange`
   * is not always reliable on Firefox for selections made by dragging
   * the cursor inside a contenteditable / styled span. mouseup is
   * cheap and only fires once per release, so we don't debounce.
   */
  useEffect(() => {
    if (!scrollContainer) return;
    const handleMouseUp = (e: MouseEvent) => {
      // If the user clicked inside the toolbar itself, don't recompute
      // — they'd just be dismissing us. `recompute` itself defers the
      // actual state update to the next animation frame, so we don't
      // need a manual RAF wrapper here.
      if (toolbarRef.current?.contains(e.target as Node)) return;
      recompute();
    };
    scrollContainer.addEventListener('mouseup', handleMouseUp);
    return () => scrollContainer.removeEventListener('mouseup', handleMouseUp);
  }, [scrollContainer, recompute]);

  /**
   * Dismiss on scroll: the stored rect is in viewport coordinates, so
   * even a 1px scroll desyncs the toolbar from the highlighted text.
   * Either we recompute (cheap, but the new selection might be
   * collapsed) or we hide. Hiding is the simpler and more honest
   * choice — the user moved their attention.
   */
  useEffect(() => {
    if (!scrollContainer) return;
    const handleScroll = () => setState(null);
    scrollContainer.addEventListener('scroll', handleScroll, { passive: true });
    window.addEventListener('scroll', handleScroll, { passive: true });
    return () => {
      scrollContainer.removeEventListener('scroll', handleScroll);
      window.removeEventListener('scroll', handleScroll);
    };
  }, [scrollContainer]);

  /**
   * Dismiss on outside click: clicks on the toolbar itself are
   * already handled by the per-button onMouseDown handlers, so we
   * only need to react to clicks elsewhere.
   */
  useEffect(() => {
    if (!state) return;
    const handlePointerDown = (e: PointerEvent) => {
      if (toolbarRef.current?.contains(e.target as Node)) return;
      setState(null);
    };
    document.addEventListener('pointerdown', handlePointerDown);
    return () => document.removeEventListener('pointerdown', handlePointerDown);
  }, [state]);

  /**
   * Keyboard handlers: Escape dismisses, Enter triggers the focused
   * preset. We don't move focus to the toolbar itself — doing so
   * would steal focus from the message area and cause the browser
   * to collapse the user's selection to the focused node, which
   * visually reads as "the selection dropped to one character".
   * The toolbar is operated via a document-level keydown listener
   * that's already in scope.
   */
  useEffect(() => {
    if (!state) return;
    setFocusedIndex(0);
  }, [state]);

  useEffect(() => {
    if (!state) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't intercept keys while the user is typing in an input
      // control. Otherwise our arrow-key shortcuts would steal
      // text-editing affordances the second the toolbar is up.
      const target = e.target as Element | null;
      if (target && target.closest('textarea, input, [contenteditable="true"]')) {
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setState(null);
        window.getSelection()?.removeAllRanges();
        return;
      }
      if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
        e.preventDefault();
        setFocusedIndex((i) => (i + 1) % PRESETS.length);
        return;
      }
      if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
        e.preventDefault();
        setFocusedIndex((i) => (i - 1 + PRESETS.length) % PRESETS.length);
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        const preset = PRESETS[focusedIndex];
        if (preset) {
          void firePreset(preset.id, stateRef.current?.selectedText ?? '');
        }
      }
    };
    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
    // focusedIndex is intentionally omitted: it's a closure concern
    // only. Including it would rebind the listener on every keypress,
    // which is wasteful but harmless; ESLint's rule of hooks prefers
    // the explicit omission so the listener respects the focus arrow
    // semantics handled by the buttons themselves.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state, onSend, disabled]);

  const firePreset = useCallback(
    async (id: PresetId, quote: string) => {
      const preset = PRESETS.find((p) => p.id === id);
      if (!preset || !quote) return;
      const prompt = preset.buildPrompt(quote);
      // Clear the live selection so the user sees the action took
      // effect, then send.
      window.getSelection()?.removeAllRanges();
      setState(null);
      try {
        await onSend(prompt);
      } catch (err) {
        // We don't want a single failed send to leave the toolbar
        // stuck around or crash the panel. The composer's own
        // notification system already surfaces errors to the user.
        console.warn('[selection-quick-actions] send failed:', err);
      }
    },
    [onSend],
  );

  if (!state) return null;

  /**
   * Position the toolbar above the selection if there's room,
   * otherwise below. We use `position: fixed` and viewport coords
   * because the scroll container can be deeply nested with arbitrary
   * transforms upstream — fixed positioning is the one anchor we know
   * is correct without traversing the parent chain.
   */
  const VIEWPORT_GAP = 8;
  const TOOLBAR_ESTIMATED_HEIGHT = 36;
  const spaceAbove = state.rect.top;
  const placeAbove = spaceAbove > TOOLBAR_ESTIMATED_HEIGHT + VIEWPORT_GAP;
  const top = placeAbove
    ? Math.max(VIEWPORT_GAP, state.rect.top - TOOLBAR_ESTIMATED_HEIGHT - VIEWPORT_GAP)
    : state.rect.bottom + VIEWPORT_GAP;
  // Centre horizontally on the selection, clamped to the viewport.
  const TOOLBAR_ESTIMATED_WIDTH = 280;
  const left = Math.max(
    VIEWPORT_GAP,
    Math.min(
      window.innerWidth - TOOLBAR_ESTIMATED_WIDTH - VIEWPORT_GAP,
      state.rect.left + state.rect.width / 2 - TOOLBAR_ESTIMATED_WIDTH / 2,
    ),
  );

  return (
    <div
      ref={toolbarRef}
      className={styles.toolbar}
      data-placement={placeAbove ? 'above' : 'below'}
      style={{ top, left }}
      role="toolbar"
      aria-label="AI 选区快捷操作"
      tabIndex={-1}
    >
      {PRESETS.map((preset, idx) => (
        <button
          key={preset.id}
          type="button"
          className={styles.action}
          data-focused={idx === focusedIndex || undefined}
          // Prevent the toolbar's pointerdown from being treated as
          // an "outside click" that dismisses us mid-selection.
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => firePreset(preset.id, state.selectedText)}
          onMouseEnter={() => setFocusedIndex(idx)}
        >
          <span className={styles.icon}>{preset.icon}</span>
          <span className={styles.label}>{preset.label}</span>
        </button>
      ))}
      <span className={styles.divider} aria-hidden="true" />
      <button
        type="button"
        className={styles.dismiss}
        onPointerDown={(e) => e.stopPropagation()}
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => {
          window.getSelection()?.removeAllRanges();
          setState(null);
        }}
        title="关闭 (Esc)"
        aria-label="关闭"
      >
        <X size={12} />
      </button>
    </div>
  );
};
