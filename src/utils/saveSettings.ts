import { invoke } from '@tauri-apps/api/core';
import type { Settings } from '../types';
import { toBackendSettings } from './settings';

export async function saveSettings(settings: Settings): Promise<void> {
  await invoke('save_settings', { settings: toBackendSettings(settings) });
}
