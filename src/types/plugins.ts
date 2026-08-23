export interface PluginManifest {
  schema_version: 1;
  id: string;
  name: string;
  version: string;
  description: string;
  prompt_path: string;
  knowledge_files: string[];
}

export interface InstalledPlugin {
  manifest: PluginManifest;
  enabled: boolean;
  installedAtUnixMs: number;
  packageSha256: string;
  knowledgeFileCount: number;
}

export interface PluginCreateInput {
  id: string;
  name: string;
  version: string;
  description: string;
  prompt: string;
  knowledgePaths: string[];
  outputPath: string;
}

export interface PluginPackageResult {
  path: string;
  manifest: PluginManifest;
  sha256: string;
  sizeBytes: number;
}
