import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useNotificationStore } from '../store';
import { reportError } from '../utils/errors';

type Unlisten = () => void;

export function useTauriEvent<TPayload>(
  eventName: string,
  handler: (payload: TPayload) => void,
) {
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  useEffect(() => {
    let unlisten: Unlisten | null = null;
    let disposed = false;

    void listen<TPayload>(eventName, (event) => {
      handler(event.payload);
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch((error) => {
        if (disposed) {
          return;
        }

        const message = reportError(`tauri-event-${eventName}`, error);
        pushNotification({
          kind: 'error',
          title: `监听事件失败：${eventName}`,
          message,
        });
      });

    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [eventName, handler, pushNotification]);
}
