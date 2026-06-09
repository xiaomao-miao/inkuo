import { create } from 'zustand';

export interface NotificationItem {
  id: string;
  title: string;
  message?: string;
  kind: 'error' | 'success' | 'info';
}

interface NotificationStore {
  notifications: NotificationItem[];
  pushNotification: (notification: Omit<NotificationItem, 'id'>) => string | null;
  dismissNotification: (id: string) => void;
  clearNotifications: () => void;
}

const NOTIFICATION_DEDUPE_WINDOW_MS = 3000;
const recentNotificationTimestamps = new Map<string, number>();

function createNotificationId() {
  return `notice-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function getNotificationKey(notification: Omit<NotificationItem, 'id'>): string {
  return `${notification.kind}::${notification.title}::${notification.message ?? ''}`;
}

function pruneRecentNotifications(now: number) {
  for (const [key, timestamp] of recentNotificationTimestamps.entries()) {
    if (now - timestamp > NOTIFICATION_DEDUPE_WINDOW_MS) {
      recentNotificationTimestamps.delete(key);
    }
  }
}

export const useNotificationStore = create<NotificationStore>((set, get) => ({
  notifications: [],
  pushNotification: (notification) => {
    const key = getNotificationKey(notification);
    const now = Date.now();
    pruneRecentNotifications(now);

    const existingVisible = get().notifications.some((item) => getNotificationKey(item) === key);
    const lastTimestamp = recentNotificationTimestamps.get(key);
    const isThrottled = typeof lastTimestamp === 'number' && now - lastTimestamp < NOTIFICATION_DEDUPE_WINDOW_MS;

    if (existingVisible || isThrottled) {
      recentNotificationTimestamps.set(key, now);
      return null;
    }

    const id = createNotificationId();
    recentNotificationTimestamps.set(key, now);
    set((state) => ({
      notifications: [...state.notifications, { ...notification, id }],
    }));
    return id;
  },
  dismissNotification: (id) => set((state) => ({
    notifications: state.notifications.filter((notification) => notification.id !== id),
  })),
  clearNotifications: () => set({ notifications: [] }),
}));
