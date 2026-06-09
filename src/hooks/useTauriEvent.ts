import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';

type Unlisten = () => void;

export function useTauriEvent<TPayload>(
  eventName: string,
  handler: (payload: TPayload) => void,
) {
  useEffect(() => {
    let unlisten: Unlisten | null = null;

    listen<TPayload>(eventName, (event) => {
      handler(event.payload);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((error) => {
        console.error(`Failed to listen for ${eventName}:`, error);
      });

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [eventName, handler]);
}
