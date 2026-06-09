import { AlertCircle, CheckCircle2, Info, X } from 'lucide-react';
import { useEffect } from 'react';
import { useNotificationStore } from '../../store/notificationStore';
import styles from './NotificationCenter.module.css';

const ICONS = {
  error: AlertCircle,
  success: CheckCircle2,
  info: Info,
} as const;

export const NotificationCenter = () => {
  const notifications = useNotificationStore((state) => state.notifications);
  const dismissNotification = useNotificationStore((state) => state.dismissNotification);

  useEffect(() => {
    if (notifications.length === 0) {
      return;
    }

    const timers = notifications.map((notification) => window.setTimeout(() => {
      dismissNotification(notification.id);
    }, 5000));

    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [notifications, dismissNotification]);

  if (notifications.length === 0) {
    return null;
  }

  return (
    <div className={styles.container} aria-live="polite" aria-atomic="true">
      {notifications.map((notification) => {
        const Icon = ICONS[notification.kind];
        return (
          <div key={notification.id} className={styles.toast} data-kind={notification.kind} role="status">
            <div className={styles.iconWrap}>
              <Icon size={16} />
            </div>
            <div className={styles.content}>
              <div className={styles.title}>{notification.title}</div>
              {notification.message && <div className={styles.message}>{notification.message}</div>}
            </div>
            <button
              type="button"
              className={styles.closeButton}
              onClick={() => dismissNotification(notification.id)}
              aria-label="关闭通知"
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
};
