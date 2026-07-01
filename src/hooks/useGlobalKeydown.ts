import { useEffect, useRef } from 'react';

export function useGlobalKeydown(handler: (event: KeyboardEvent) => void) {
  // Hold the latest handler in a ref so the effect registers the listener
  // exactly once per mount. Without this, callers that pass an inline
  // lambda (most callers) would re-register on every render — wasting work
  // and creating a window in which both the old and new listener are
  // attached during the cleanup/setup cycle.
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    const listener = (event: KeyboardEvent) => handlerRef.current(event);
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, []);
}
