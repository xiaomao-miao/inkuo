// Per-tool field-label schema used by `formatArgumentsForDisplay`.
//
// Each entry lists the argument keys (in display priority order) that the
// AI panel shows for that tool, plus the Chinese label that should
// replace the key in the rendered output. Fields marked `summarize` use
// a special formatter for nested-array values whose raw JSON would be
// unreadable (Word `elements`, Excel `operations`, Excel `sheets`).
//
// Split out of the original monolithic `toolUtils.ts` so the schema is
// easy to scan / edit and keeps its sibling modules (streaming extractors,
// renderers) focused on logic rather than data.

type ToolField = { key: string; label: string; summarize?: 'elements' | 'operations' | 'sheets' };

export const TOOL_FIELD_LABELS: Record<string, ToolField[]> = {
  grep: [
    { key: 'pattern', label: '搜索' },
    { key: 'path', label: '路径' },
    { key: 'file_pattern', label: '文件类型' },
  ],
  glob: [
    { key: 'pattern', label: '匹配' },
    { key: 'path', label: '目录' },
  ],
  list_dir: [
    { key: 'path', label: '目录' },
  ],
  read_file: [
    { key: 'path', label: '文件' },
    { key: 'start_line', label: '起始行' },
    { key: 'end_line', label: '结束行' },
  ],
  read_office_file: [
    { key: 'path', label: '文件' },
  ],
  create_dir: [
    { key: 'dir_path', label: '目录' },
    { key: 'directory', label: '目录' },
  ],
  move_file: [
    { key: 'source_path', label: '源文件' },
    { key: 'source', label: '源文件' },
    { key: 'destination', label: '目标' },
  ],
  database_search: [
    { key: 'query', label: '查询' },
    { key: 'top_k', label: '结果数' },
  ],
  update_todo: [
    { key: 'action', label: '操作' },
    { key: 'items', label: '任务' },
  ],
  ask_user: [
    { key: 'question', label: '问题' },
    { key: 'options', label: '选项' },
  ],
  delegate_to: [
    { key: 'expert', label: '专家' },
    { key: 'task', label: '任务' },
    { key: 'context', label: '背景' },
  ],
  // File tools
  write_file: [
    { key: 'path', label: '文件' },
    { key: 'content', label: '内容' },
  ],
  edit_file: [
    { key: 'path', label: '文件' },
    { key: 'old_text', label: '原文' },
    { key: 'new_text', label: '替换为' },
  ],
  // Office: Word
  create_word_doc: [
    { key: 'path', label: '文件' },
    { key: 'title', label: '标题' },
    { key: 'elements', label: '正文', summarize: 'elements' },
  ],
  compare_word_docs: [
    { key: 'path1', label: '原文档' },
    { key: 'path2', label: '新文档' },
  ],
  // Office: Excel (unified backend tools)
  modify_excel: [
    { key: 'path', label: '文件' },
    { key: 'operations', label: '操作', summarize: 'operations' },
  ],
  create_excel: [
    { key: 'path', label: '文件' },
    { key: 'sheets', label: '内容', summarize: 'sheets' },
  ],
  inspect_office: [
    { key: 'path', label: '文件' },
    { key: 'format', label: '格式' },
    { key: 'mode', label: '模式' },
    { key: 'sheet', label: '工作表' },
    { key: 'range', label: '区域' },
  ],
  // Image / vector tools
  create_svg: [
    { key: 'description', label: '描述' },
    { key: 'output_path', label: '保存到' },
    { key: 'aspect_ratio', label: '比例' },
  ],
  create_pptx: [
    { key: 'output_path', label: '保存到' },
    { key: 'title', label: '标题' },
  ],
  generate_image: [
    { key: 'prompt', label: '描述' },
    { key: 'output_path', label: '保存到' },
    { key: 'width', label: '宽度' },
    { key: 'height', label: '高度' },
    { key: 'model', label: '模型' },
    { key: 'negative_prompt', label: '反向提示' },
  ],
};

/**
 * Keys that, when listed alongside an earlier match, should suppress the
 * later copy of the value. The schema above intentionally lists both
 * `dir_path` and `directory` (and `source_path` and `source`) so that
 * tools can use either, but the renderer must only show one line per
 * "logical" field.
 */
export const PATH_DEDUP_KEYS: ReadonlySet<string> = new Set([
  'dir_path',
  'directory',
  'source_path',
  'source',
]);
