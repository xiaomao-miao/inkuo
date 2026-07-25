// Tool / expert display-name registries.
//
// Small lookup tables + helpers that translate the short tool / expert
// identifiers used internally into their human-readable Chinese names
// shown in the AI panel UI. The tables double as the source of truth for
// "what tools are available" — consumers like the chat input's feature
// toolbar can iterate `COMPACT_TOOLS` to know which tools render the
// compact card variant, etc.
//
// Split out of the original monolithic `toolUtils.ts` so the registries
// don't get mixed in with the heavier rendering / streaming-extraction
// logic.

const TOOL_DISPLAY_NAMES: Record<string, string> = {
  read_file: '读取文件',
  write_file: '写入文件',
  edit_file: '编辑文件',
  list_dir: '列出目录',
  glob: '查找文件',
  grep: '搜索文本',
  read_office_file: '读取 Office 文件',
  create_word_doc: '创建 Word 文档',
  compare_word_docs: '比较 Word 文档',
  create_dir: '创建目录',
  knowledge_build: '构建知识库',
  move_file: '移动文件',
  database_search: '搜索知识库',
  // Office tools (unified backend)
  modify_excel: '修改 Excel',
  create_excel: '创建 Excel',
  inspect_office: '检查 Office 文件',
  // Image / vector tools
  create_svg: '生成 SVG 图片',
  create_pptx: '生成 PPT',
  generate_image: '生成图片',
  // Meta / sub-agent tools
  get_tool_help: '加载工具帮助',
  delegate_to: '委派子代理',
  update_todo: '更新任务列表',
  ask_user: '向用户提问',
};

const EXPERT_DISPLAY_NAMES: Record<string, string> = {
  office_word_expert: 'Word 文档专家',
  office_excel_expert: 'Excel 文档专家',
  office_pptx_expert: 'PPT 演示专家',
  md_writer: 'Markdown 写作专家',
  researcher: '调研员',
  batch_editor: '批量编辑员',
  code_expert: '代码工程专家',
};

export function getExpertDisplayName(name: string): string {
  return EXPERT_DISPLAY_NAMES[name] || name;
}

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
  'modify_excel',
  'create_excel',
  'create_pptx',
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
  // Split on both POSIX '/' and Windows '\' so a path like
  // 'C:\Users\me\report.docx' works the same as '/home/me/report.docx'.
  const parts = path.split(/[/\\]/).filter((p) => p.length > 0);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}
