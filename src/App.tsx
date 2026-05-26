import { useEffect } from 'react';
import { Layout } from './components/layout';
import { CmdK } from './components/cmdk';
import { useSettingsStore, useSidebarStore } from './store';

import './styles/design-tokens.css';
import './styles/global.css';

function App() {
  const { settings } = useSettingsStore();
  const { openTabs, activeTabId, setActiveTab } = useSidebarStore();

  // Apply theme
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', settings.theme);

    // Apply accent color as CSS variable
    document.documentElement.style.setProperty('--accent-primary', settings.accent_color);
    document.documentElement.style.setProperty('--accent-hover', adjustColor(settings.accent_color, 20));
    document.documentElement.style.setProperty('--accent-active', adjustColor(settings.accent_color, -20));
  }, [settings.theme, settings.accent_color]);

  // Restore open tabs on startup
  useEffect(() => {
    if (openTabs.length > 0 && activeTabId) {
      setActiveTab(activeTabId);
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <>
      <Layout />
      <CmdK />
    </>
  );
}

// Helper function to adjust color brightness
function adjustColor(hex: string, percent: number): string {
  const num = parseInt(hex.replace('#', ''), 16);
  const amt = Math.round(2.55 * percent);
  const R = Math.min(255, Math.max(0, (num >> 16) + amt));
  const G = Math.min(255, Math.max(0, ((num >> 8) & 0x00FF) + amt));
  const B = Math.min(255, Math.max(0, (num & 0x0000FF) + amt));
  return `#${(0x1000000 + R * 0x10000 + G * 0x100 + B).toString(16).slice(1)}`;
}

export default App;
