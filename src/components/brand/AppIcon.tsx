import type { CSSProperties } from 'react';

interface AppIconProps {
  size?: number;
  className?: string;
  style?: CSSProperties;
}

/**
 * 应用图标 —— 原 inkUO.svg 的 React 内嵌版。
 * 复用 src-tauri/icons/icon.svg 同一份视觉资产:
 *   - 深色圆角方块 + 右侧青色折叠丝带 + 正面白色面板
 *   - 1280x800 视口里 22px 仍能看出镂空
 * 在 Wordmark / TitleBar / 未来其它需要小图标的地方共用。
 */
export const AppIcon: React.FC<AppIconProps> = ({ size = 22, className, style }) => (
  <svg
    className={className}
    width={size}
    height={size}
    viewBox="0 0 1024 1024"
    role="img"
    aria-label="inkuo"
    style={style}
  >
    <defs>
      <linearGradient id="inkuoIconBg" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor="#2a3035" />
        <stop offset="0.42" stopColor="#11171a" />
        <stop offset="1" stopColor="#020708" />
      </linearGradient>
      <radialGradient id="inkuoIconGlow" cx="88%" cy="86%" r="65%">
        <stop offset="0" stopColor="#00c8d1" stopOpacity="0.25" />
        <stop offset="0.52" stopColor="#006a72" stopOpacity="0.07" />
        <stop offset="1" stopColor="#000000" stopOpacity="0" />
      </radialGradient>
      <linearGradient id="inkuoIconEdge" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor="#ffffff" stopOpacity="0.76" />
        <stop offset="0.2" stopColor="#ffffff" stopOpacity="0.08" />
        <stop offset="0.68" stopColor="#23eef1" stopOpacity="0.2" />
        <stop offset="1" stopColor="#55f5f6" stopOpacity="0.8" />
      </linearGradient>
      <linearGradient id="inkuoIconWing" x1="0.2" y1="0" x2="0.82" y2="1">
        <stop offset="0" stopColor="#4ff5f4" />
        <stop offset="0.45" stopColor="#19dbe3" />
        <stop offset="1" stopColor="#02aebb" />
      </linearGradient>
      <linearGradient id="inkuoIconFold" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0" stopColor="#007d89" />
        <stop offset="0.55" stopColor="#009ca9" />
        <stop offset="1" stopColor="#15c9d2" />
      </linearGradient>
      <linearGradient id="inkuoIconPanel" x1="0" y1="0" x2="0.75" y2="1">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset="0.72" stopColor="#f8fafb" />
        <stop offset="1" stopColor="#e9eef0" />
      </linearGradient>
    </defs>
    <rect x="32" y="28" width="960" height="960" rx="212" fill="url(#inkuoIconBg)" />
    <rect x="32" y="28" width="960" height="960" rx="212" fill="url(#inkuoIconGlow)" />
    <rect
      x="35"
      y="31"
      width="954"
      height="954"
      rx="209"
      fill="none"
      stroke="url(#inkuoIconEdge)"
      strokeWidth="5"
    />
    <path
      d="M456 520 L691 285 C714 262 750 276 746 308 L712 575 C709 600 701 616 683 633 L513 793 C489 816 456 799 456 765 Z"
      fill="url(#inkuoIconWing)"
    />
    <path
      d="M456 520 L505 471 L505 738 C505 759 497 775 482 790 L470 801 C461 792 456 780 456 765 Z"
      fill="url(#inkuoIconFold)"
    />
    <path
      d="M305 275 H435 C460 275 480 295 480 320 V740 C480 765 460 785 435 785 H305 C280 785 260 765 260 740 V320 C260 295 280 275 305 275 Z"
      fill="url(#inkuoIconPanel)"
    />
  </svg>
);
