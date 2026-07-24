import { AppIcon } from '../brand/AppIcon';
import styles from './WelcomePage.module.css';

/**
 * 品牌字标 —— inkUO 应用图标 + 文字。
 * 图形部分直接复用 src-tauri/icons 的 SVG 视觉,但这里保持单色前景
 * (`currentColor`) 不可行 —— 原版有渐变,所以直接嵌入 AppIcon。
 */
export const Wordmark: React.FC<{ size?: number }> = ({ size = 40 }) => (
  <div className={styles.wordmark}>
    <AppIcon size={size} className={styles.symbol} />
    <span className={styles.wordmarkText}>inkuo</span>
  </div>
);
