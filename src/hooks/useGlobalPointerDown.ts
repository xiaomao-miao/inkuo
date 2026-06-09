import { useEffect } from 'react';

export function useGlobalPointerDown(
  handler: (event: PointerEvent) => void,
  options?: AddEventListenerOptions | boolean,
) {
  useEffect(() => {
    window.addEventListener('pointerdown', handler, options);
    return () => window.removeEventListener('pointerdown', handler, options);
  }, [handler, options]);
}
