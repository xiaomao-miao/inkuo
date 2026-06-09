import { useCallback, useEffect, useRef } from 'react';

export function useDebouncedCallback<T extends (...args: Parameters<T>) => void>(
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
        callbackRef.current(...pendingRef.current);
        pendingRef.current = null;
      }
    }, delayRef.current);
  }, []) as T;
}
