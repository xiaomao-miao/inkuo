import { SETTINGS_TAB_ID, useSidebarStore } from '../store';

export function openSettingsTab(): void {
  useSidebarStore.getState().openTab({
    id: SETTINGS_TAB_ID,
    path: SETTINGS_TAB_ID,
    name: '设置',
    isDirty: false,
    isSettings: true,
  });
}
