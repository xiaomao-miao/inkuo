import { invoke } from '@tauri-apps/api/core';

import type {
  InstalledPlugin,
  PluginCreateInput,
  PluginPackageResult,
} from '../types/plugins';

export function listPlugins(): Promise<InstalledPlugin[]> {
  return invoke<InstalledPlugin[]>('plugin_list');
}

export function createPluginPackage(input: PluginCreateInput): Promise<PluginPackageResult> {
  return invoke<PluginPackageResult>('plugin_create_package', { input });
}

export function importPlugin(packagePath: string): Promise<InstalledPlugin> {
  return invoke<InstalledPlugin>('plugin_import', { packagePath });
}

export function setPluginEnabled(pluginId: string, enabled: boolean): Promise<InstalledPlugin> {
  return invoke<InstalledPlugin>('plugin_set_enabled', { pluginId, enabled });
}

export function exportPlugin(pluginId: string, outputPath: string): Promise<PluginPackageResult> {
  return invoke<PluginPackageResult>('plugin_export', { pluginId, outputPath });
}

export function removePlugin(pluginId: string): Promise<void> {
  return invoke<void>('plugin_remove', { pluginId });
}
