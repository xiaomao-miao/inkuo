import { useCallback, useEffect, useRef, useState } from "react";
import { EDIT_HANDLE_ATTR, KIND_BY_TAG } from "./types";
import { findWrapperFor } from "./parseSvg";

/**
 * Manage which wrapper `<g>` is currently selected. Returns the live
 * `SVGGElement` (so callers can read its bbox / transform directly)
 * and a setter that walks `pointerdown` events on the SVG to find the
 * right ancestor.
 *
 * We don't model selection with React refs of child components
 * because the SVG tree is mounted via `dangerouslySetInnerHTML`. The
 * DOM refs are the source of truth.
 */

export interface SelectionState {
  wrapper: SVGGElement | null;
  /** The original (inner) selectable element inside the wrapper. */
  original: SVGElement | null;
}

export function useSelection(svgRef: React.RefObject<SVGSVGElement | null>) {
  const [state, setState] = useState<SelectionState>({ wrapper: null, original: null });
  // The wrapper is mutated in place during drag (we write
  // `transform` directly on it). Use a ref so pointermove handlers
  // always see the latest value even if React batches state updates.
  const stateRef = useRef(state);
  stateRef.current = state;

  const clear = useCallback(() => {
    setState({ wrapper: null, original: null });
  }, []);

  const selectFromEvent = useCallback(
    (target: EventTarget | null) => {
      if (!(target instanceof Element)) return;
      // Walk up from the event target to find a wrapper, otherwise the
      // inner element so we know if it was a direct hit.
      let node: Element | null = target;
      while (node && node !== svgRef.current) {
        if (node instanceof SVGElement) {
          const wrapper = findWrapperFor(node);
          if (wrapper) {
            const inner = Array.from(wrapper.children).find(
              (c) => (c as Element).getAttribute?.(EDIT_HANDLE_ATTR) === null,
            ) as SVGElement | undefined;
            setState({
              wrapper: wrapper as SVGGElement,
              original: inner ?? null,
            });
            return;
          }
        }
        node = node.parentElement;
      }
      // Click landed on the SVG background → clear selection.
      clear();
    },
    [svgRef, clear],
  );

  const selectById = useCallback((id: string) => {
    if (!svgRef.current) return;
    const node = svgRef.current.querySelector(`g#${CSS.escape(id)}[${EDIT_HANDLE_ATTR}]`);
    if (!node) return;
    const wrapper = node as SVGGElement;
    const inner = Array.from(wrapper.children).find(
      (c) => (c as Element).getAttribute?.(EDIT_HANDLE_ATTR) === null,
    ) as SVGElement | undefined;
    setState({ wrapper, original: inner ?? null });
  }, [svgRef]);

  /** Programmatic selection (e.g. from keyboard nav). */
  const selectWrapper = useCallback((wrapper: SVGGElement | null) => {
    if (!wrapper) {
      clear();
      return;
    }
    const inner = Array.from(wrapper.children).find(
      (c) => (c as Element).getAttribute?.(EDIT_HANDLE_ATTR) === null,
    ) as SVGElement | undefined;
    setState({ wrapper, original: inner ?? null });
  }, [clear]);

  /** Auto-clear if the currently selected wrapper was removed from the DOM. */
  useEffect(() => {
    if (!state.wrapper) return;
    const observer = new MutationObserver(() => {
      if (!state.wrapper || !state.wrapper.isConnected) {
        clear();
      }
    });
    observer.observe(svgRef.current ?? document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, [state.wrapper, clear, svgRef]);

  /**
   * Whether the currently-selected element supports a given interaction.
   * Centralized here so the overlay and TextEditor agree.
   */
  const capabilities = (() => {
    if (!state.original) return null;
    return KIND_BY_TAG[state.original.localName] ?? null;
  })();

  return {
    state,
    stateRef,
    selectFromEvent,
    selectById,
    selectWrapper,
    clear,
    capabilities,
  };
}

export type UseSelectionReturn = ReturnType<typeof useSelection>;
