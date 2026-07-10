import { useSettingsStore } from '../../store';
import { CloudAuthPanel } from './CloudAuthPanel';
import { CloudAccountCard } from './CloudAccountCard';
import styles from './CloudPanel.module.css';

/**
 * Top-level switcher that picks between the auth panel (no account)
 * and the account card (signed in). Kept thin so each panel can be
 * rendered elsewhere if needed.
 */
export const CloudPanel = () => {
  const account = useSettingsStore((s) => s.settings.cloud.account);

  return (
    <div className={styles.cloudPanel}>
      {account ? <CloudAccountCard /> : <CloudAuthPanel />}
    </div>
  );
};