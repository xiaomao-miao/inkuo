const TOOL_DISPLAY_NAMES: Record<string, string> = {
  read_file: '读取文件',
  write_file: '写入文件',
  edit_file: '编辑文件',
  list_dir: '列出目录',
  glob: '查找文件',
  grep: '搜索文本',
  read_office_file: '读取 Office 文件',
  create_word_doc: '创建 Word 文档',
  create_dir: '创建目录',
  knowledge_build: '构建知识库',
  move_file: '移动文件',
  database_search: '搜索知识库',
};

export function getToolDisplayName(name: string): string {
  return TOOL_DISPLAY_NAMES[name] || name;
}

export const COMPACT_TOOLS = new Set([
  'list_dir',
  'glob',
  'grep',
  'read_file',
  'read_office_file',
  'create_dir',
  'move_file',
]);

export const FILE_MODIFICATION_TOOLS = new Set([
  'write_file',
  'edit_file',
  'create_word_doc',
]);

export const PREVIEW_STRING_KEYS = new Set([
  'content',
  'new_text',
  'pattern',
  'json_content',
]);

export function isFileModificationTool(name: string): boolean {
  return FILE_MODIFICATION_TOOLS.has(name);
}

export function extractFileNameFromPath(path: string | undefined | null): string | null {
  if (!path) return null;
  return path.split('/').pop() || path.split('\\').pop() || path;
}
