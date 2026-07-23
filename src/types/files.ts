// File-system / viewer types — FileKind, FileEntry, ViewerFilePayload
// and the helpers used to derive FileKind from a path. Also houses
// the new-file template presets used by the context-menu flow.

/**
 * Coarse-grained classification of a file's display mode. Drives the
 * editor's renderer (Image / Pdf / Code / Text / Config / Data), the
 * sidebar icon, and the AI agent's choice of read-tool.
 *
 *   - `word` / `excel`: existing office editors (no change)
 *   - `image`: raster + SVG, displayed by `ImageViewer`
 *   - `pdf`: PDF, displayed by `PdfViewer` (pdf.js)
 *   - `code`: source files with syntax highlighting inside CodeMirror
 *   - `config`: structured text (JSON/YAML/TOML/XML) with syntax highlighting
 *   - `data`: tabular data (CSV/TSV)
 *   - `markdown`: the existing markdown editor
 *   - `text`: plain text (fallback)
 *   - `binary`: unknown / unsupported binary formats
 */
export type FileKind =
  | 'word'
  | 'excel'
  | 'image'
  | 'pdf'
  | 'code'
  | 'config'
  | 'data'
  | 'markdown'
  | 'text'
  | 'binary'
  | 'audio'
  | 'video'
  | 'archive';

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  /** @deprecated kept for backwards compatibility; use `file_kind` instead. */
  is_markdown: boolean;
  /** Coarse-grained classification driving the editor + icon mapping. */
  file_kind: FileKind;
}

/**
 * Payload returned by the `read_file_for_viewer` Tauri command. Mirrors
 * `commands::ViewerFilePayload` (Rust).
 */
export interface ViewerFilePayload {
  path: string;
  size: number;
  /** Best-effort MIME type derived from the file extension. */
  mime: string;
  /** Coarse-grained `FileKind` classification (matches `FileEntry.file_kind`). */
  file_kind: FileKind;
  /** Raw file bytes encoded as base64. */
  data_base64: string;
}

/** Payload for the `create_file_entry` Rust command. */
export type NewEntryPayload =
  | { kind: 'file'; extension: string; template?: string }
  | { kind: 'directory' };

export interface CreateEntryResult {
  path: string;
}

export interface RenamePathResult {
  from: string;
  to: string;
}

/** Built-in template presets for new-file creation in the context menu. */
export interface NewFileTemplate {
  id: string;
  label: string;
  extension: string;
  template: string;
}

export const NEW_FILE_TEMPLATES: readonly NewFileTemplate[] = [
  { id: 'md', label: 'Markdown', extension: 'md', template: '# 无标题\n\n开始书写…\n' },
  { id: 'txt', label: '纯文本', extension: 'txt', template: '' },
  { id: 'docx', label: 'Word 文档', extension: 'docx', template: '' },
  { id: 'xlsx', label: 'Excel 工作簿', extension: 'xlsx', template: '' },
] as const;

/**
 * Map a filename (or full path) to a coarse-grained `FileKind`.
 *
 * This is the **single source of truth** used by:
 *   - The editor router (`Editor.tsx`) to pick which viewer renders.
 *   - The sidebar (`FileTree.tsx` / `Sidebar.tsx`) to pick an icon.
 *   - The tab bar (`TabBar.tsx`) to pick an icon.
 *   - The AI agent to decide which read-tool to invoke.
 *
 * Extension matching is case-insensitive. The function tolerates paths
 * (e.g. `C:\foo\bar.png` or `/tmp/x.json`) and only inspects the final
 * extension segment.
 */
export function detectFileKind(input: string): FileKind {
  // Strip path and grab the last `.ext` segment.
  const base = input.split(/[\\/]/).pop() ?? input;
  const dot = base.lastIndexOf('.');
  if (dot < 0 || dot === base.length - 1) return 'text';
  const ext = base.slice(dot + 1).toLowerCase();

  // Office
  if (ext === 'docx' || ext === 'doc') return 'word';
  if (ext === 'xlsx' || ext === 'xls' || ext === 'xlsm' || ext === 'csv') return 'excel';

  // Documents (rendered by markdown editor)
  if (ext === 'md' || ext === 'markdown') return 'markdown';

  // PDF
  if (ext === 'pdf') return 'pdf';

  // Images (raster + vector)
  if (
    ext === 'png' ||
    ext === 'jpg' ||
    ext === 'jpeg' ||
    ext === 'gif' ||
    ext === 'webp' ||
    ext === 'bmp' ||
    ext === 'ico' ||
    ext === 'avif' ||
    ext === 'tif' ||
    ext === 'tiff' ||
    ext === 'svg'
  ) {
    return 'image';
  }

  // Structured config
  if (
    ext === 'json' ||
    ext === 'jsonc' ||
    ext === 'json5' ||
    ext === 'yaml' ||
    ext === 'yml' ||
    ext === 'toml' ||
    ext === 'ini' ||
    ext === 'xml' ||
    ext === 'env'
  ) {
    return 'config';
  }

  // Tabular data
  if (ext === 'csv' || ext === 'tsv') return 'data';

  // Source code (CodeMirror language-data covers all of these and more)
  const CODE_EXTS = new Set([
    'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs',
    'rs', 'py', 'go', 'java', 'kt', 'swift',
    'c', 'h', 'cpp', 'cc', 'cxx', 'hpp', 'hxx',
    'rb', 'php', 'lua', 'sh', 'bash', 'zsh',
    'sql', 'graphql', 'gql',
    'html', 'htm', 'css', 'scss', 'sass', 'less',
    'vue', 'svelte', 'astro',
    'dart', 'r', 'jl', 'pl', 'scala', 'clj',
    'ex', 'exs', 'erl', 'hs', 'ml', 'fs', 'fsx',
    'mdx', 'vue', 'svelte',
  ]);
  if (CODE_EXTS.has(ext)) return 'code';

  // Plain text fallback
  if (ext === 'txt' || ext === 'log' || ext === 'text') return 'text';

  // Media (declared but currently only show metadata + "open with system app")
  if (
    ext === 'mp3' || ext === 'wav' || ext === 'flac' ||
    ext === 'aac' || ext === 'ogg' || ext === 'm4a'
  ) return 'audio';
  if (
    ext === 'mp4' || ext === 'mov' || ext === 'mkv' || ext === 'webm' ||
    ext === 'avi' || ext === 'm4v'
  ) return 'video';

  // Archives + other binaries
  if (
    ext === 'zip' || ext === 'tar' || ext === 'gz' || ext === 'tgz' ||
    ext === 'bz2' || ext === 'xz' || ext === '7z' || ext === 'rar' ||
    ext === 'jar' || ext === 'war'
  ) return 'archive';

  // Unknown binary
  return 'binary';
}

/**
 * Backwards-compatible wrapper that returns the legacy 4-way discriminator
 * used by `Editor.tsx` and `officeTabs` until the migration is finished.
 */
export function detectLegacyFileType(
  input: string,
): 'markdown' | 'plaintext' | 'word' | 'excel' {
  const kind = detectFileKind(input);
  switch (kind) {
    case 'word':
      return 'word';
    case 'excel':
      return 'excel';
    case 'markdown':
      return 'markdown';
    case 'image':
    case 'pdf':
      return 'plaintext'; // legacy callers shouldn't render these
    default:
      return 'plaintext';
  }
}