import { useEffect, useRef } from 'react';

export function useGlobalPointerDown(
  handler: (event: PointerEvent) => void,
  options?: AddEventListenerOptions | boolean,
) {
  // Hold the latest handler in a ref so the listener is registered exactly
  // once per mount. See `useGlobalKeydown` for the same rationale.
  const handlerRef = useRef(handler);
  handlerRef.current = handler;
  // Memo the options object identity so callers can pass a fresh object
  // each render without forcing the effect to re-run.
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useEffect(() => {
    const listener = (event: PointerEvent) => handlerRef.current(event);
    window.addEventListener('pointerdown', listener, optionsRef.current);
    return () => window.removeEventListener('pointerdown', listener, optionsRef.current);
  }, []);
}
