import { useEffect, useRef } from 'react';
import { Layout } from './components/layout';
import { CmdK } from './components/cmdk';
import { useSettingsStore, useSidebarStore } from './store';
import { adjustColor } from './utils/color';

import './styles/design-tokens.css';
import './styles/global.css';

function App() {
  const { settings } = useSettingsStore();
  const { hasRestoredFromPersist } = useSidebarStore();

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', settings.theme);

    document.documentElement.style.setProperty('--accent-primary', settings.accent_color);
    document.documentElement.style.setProperty('--accent-hover', adjustColor(settings.accent_color, 20));
    document.documentElement.style.setProperty('--accent-active', adjustColor(settings.accent_color, -20));
  }, [settings.theme, settings.accent_color]);

  const initTabRestored = useRef(false);
  useEffect(() => {
    if (hasRestoredFromPersist) {
      initTabRestored.current = true;
    }
  }, [hasRestoredFromPersist]);

  return (
    <>
      <Layout />
      <CmdK />
    </>
  );
}

export default App;
