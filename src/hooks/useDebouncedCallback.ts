import { useCallback, useEffect, useRef } from 'react';

/**
 * Coalesce a stream of calls into a single trailing-edge invocation after
 * `delayMs` of quiet. Every call replaces the previously pending args.
 *
 * This is the right tool when one side of the pipeline already guarantees
 * that all relevant work fits into the last call (e.g. a backend that emits
 * a single batched event per quiet window). For pipelines where each call
 * is a distinct piece of work that must NOT replace its siblings, use
 * `useKeyedDebouncedCallback` instead.
 */
export function useDebouncedCallback<T extends (...args: any[]) => void>(
  callback: T,
  delayMs: number
): T {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingRef = useRef<Parameters<T> | null>(null);
  const callbackRef = useRef(callback);
  const delayRef = useRef(delayMs);

  callbackRef.current = callback;
  delayRef.current = delayMs;

  useEffect(() => {
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      pendingRef.current = null;
    };
  }, []);

  return useCallback((...args: Parameters<T>) => {
    pendingRef.current = args;
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
    }

    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      if (pendingRef.current !== null) {
        callbackRef.current(...(pendingRef.current as Parameters<T>));
        pendingRef.current = null;
      }
    }, delayRef.current);
  }, []) as T;
}

/**
 * Like `useDebouncedCallback`, but maintains a separate trailing timer per
 * `extractKey(...args)` key.
 *
 * Why this exists: a flat debouncer only remembers the last call. When the
 * watcher reports changes in two different parent directories inside the
 * same debounce window, the flat debouncer silently drops the first parent
 * and only refreshes the second. A keyed debouncer keeps one pending
 * payload per key and fires each of them independently once their own
 * quiet window expires — so two parents that both need re-listing are
 * both re-listed.
 *
 * The bookkeeping shape is a `Map<K, { timer, pending }>` per debouncer
 * instance. `extractKey` runs on every call and must be cheap.
 */
export function useKeyedDebouncedCallback<K, T extends (...args: any[]) => void>(
  callback: T,
  extractKey: (args: Parameters<T>) => K,
  delayMs: number,
): (...args: Parameters<T>) => void {
  // `extractKey` flows through a ref so the returned `trigger` callback
  // is referentially stable across renders even if the caller passes a
  // fresh inline closure each frame. Same trick for `callback` /
  // `delayMs`: the debounced invocation always uses the latest values.
  const entriesRef = useRef<
    Map<K, { timer: ReturnType<typeof setTimeout> | null; pending: Parameters<T> | null }>
  >(new Map());
  const callbackRef = useRef(callback);
  const delayRef = useRef(delayMs);
  const extractKeyRef = useRef(extractKey);

  callbackRef.current = callback;
  delayRef.current = delayMs;
  extractKeyRef.current = extractKey;

  // Capture the entries Map reference at mount time so the cleanup
  // closure doesn't read through a ref that React may have swapped out
  // under our feet (the `react-hooks/exhaustive-deps` rule flags exactly
  // this pattern).
  const entriesAtMount = entriesRef.current;

  useEffect(() => {
    return () => {
      // Drop every pending timer on unmount so a fast workspace switch
      // doesn't fire callbacks against a stale store instance.
      for (const entry of entriesAtMount.values()) {
        if (entry.timer !== null) {
          clearTimeout(entry.timer);
        }
      }
      entriesAtMount.clear();
    };
    // entriesAtMount is a stable reference (the ref's `.current` at mount).
    // Listing it once is sufficient; the effect itself only runs on
    // unmount because the dep array is empty.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return useCallback((...args: Parameters<T>) => {
    const key = extractKeyRef.current(args);
    const entries = entriesRef.current;
    let entry = entries.get(key);
    if (!entry) {
      entry = { timer: null, pending: null };
      entries.set(key, entry);
    }
    entry.pending = args;
    if (entry.timer !== null) {
      clearTimeout(entry.timer);
    }
    entry.timer = setTimeout(() => {
      entry.timer = null;
      if (entry.pending !== null) {
        const pendingArgs = entry.pending as Parameters<T>;
        entry.pending = null;
        // Clean up empty entries so the Map can't grow unbounded under
        // sustained churn (e.g. an active build pipeline).
        entries.delete(key);
        callbackRef.current(...pendingArgs);
      }
    }, delayRef.current);
  }, []);
}
