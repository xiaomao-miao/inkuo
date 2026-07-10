import styles from './WelcomePage.module.css';

/**
 * 品牌字标 —— 不用圆形 emoji 字母,改用一个简单的几何符号 + 文字。
 * 笔画"i"被画成一滴墨水的形状,呼应 "inkuo / 墨" 这个品牌隐喻;
 * 符号大小写排版遵循 Lineto / Vercel 一类的克制风格。
 */
export const Wordmark: React.FC = () => (
  <div className={styles.wordmark}>
    <svg
      className={styles.symbol}
      width="40"
      height="40"
      viewBox="0 0 36 36"
      fill="none"
      aria-hidden
    >
      {/* 上半:简洁的方形外框 */}
      <rect
        x="3"
        y="3"
        width="30"
        height="30"
        rx="8"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      {/* 下半:墨滴 */}
      <path
        d="M18 11c-2.8 3.5-4.6 6-4.6 8.4a4.6 4.6 0 0 0 9.2 0c0-2.4-1.8-4.9-4.6-8.4z"
        fill="currentColor"
        opacity="0.92"
      />
    </svg>
    <span className={styles.wordmarkText}>inkuo</span>
  </div>
);
