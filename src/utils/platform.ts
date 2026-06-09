const isMacPlatform = typeof navigator !== 'undefined' && /Mac|iPhone|iPad|iPod/.test(navigator.platform);

export function getModifierKeyLabel(): 'Cmd' | 'Ctrl' {
  return isMacPlatform ? 'Cmd' : 'Ctrl';
}

export function formatShortcut(shortcut: string): string {
  return shortcut.replace(/Cmd/g, getModifierKeyLabel());
}
