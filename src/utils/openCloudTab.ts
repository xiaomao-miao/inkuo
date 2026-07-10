import { CLOUD_TAB_ID, useSidebarStore } from '../store';

export function openCloudTab(): void {
  useSidebarStore.getState().openTab({
    id: CLOUD_TAB_ID,
    path: CLOUD_TAB_ID,
    name: 'inkuo Cloud',
    isDirty: false,
    isCloud: true,
  });
}