import React, { useState, useMemo } from 'react';
import { ChevronRight, ChevronDown } from 'lucide-react';
import { COMPACT_TOOLS, getToolDisplayName, extractFileNameFromPath } from './toolRender';
import type { OutputItem } from '../../store';
import styles from './InlineCompactTool.module.css';

interface InlineCompactToolProps {
  /** The tool_call_start OutputItem. The corresponding tool_result (if any) is
   * looked up by toolCallId against `trailingItems`. */
  item: Extract<OutputItem, { type: 'tool_call_start' }>;
  /** All OutputItems in the same message, in order. Used to find the matching
   * tool_result by toolCallId. */
  trailingItems?: OutputItem[];
}

/**
 * Decide what the "headline" text of a compact tool line should be when it's
 * collapsed. Kept deliberately small — this is the human-readable summary
 * that lives on the inline line, NOT the verbose result.
 *
 *   list_dir('foo')   → "列表目录:foo"
 *   read_file('a.md') → "读取文件:a.md"
 *   grep(pattern=x)   → "搜索:x"
 *   glob('*.ts')      → "匹配:*.ts"
 *   create_dir        → "创建目录"
 *   move_file         → "移动文件"
 */
function buildHeadline(
  toolName: string,
  args: Record<string, unknown>,
): { label: string; value: string } {
  const filePath =
    (args?.path as string | undefined) ??
    (args?.file_path as string | undefined) ??
    (args?.file as string | undefined);
  const dirPath =
    (args?.dir_path as string | undefined) ??
    (args?.directory as string | undefined);
  const pattern =
    (args?.pattern as string | undefined) ??
    (args?.glob as string | undefined);
  const sourcePath =
    (args?.source_path as string | undefined) ??
    (args?.source as string | undefined);
  const destPath =
    (args?.dest_path as string | undefined) ??
    (args?.destination as string | undefined);

  switch (toolName) {
    case 'list_dir':
      return { label: '列表目录', value: dirPath ?? filePath ?? '' };
    case 'read_file':
    case 'read_office_file':
      return {
        label: toolName === 'read_office_file' ? '读取 Office 文件' : '读取文件',
        value: extractFileNameFromPath(filePath) ?? filePath ?? '',
      };
    case 'grep':
      return { label: '搜索', value: pattern ?? '' };
    case 'glob':
      return { label: '匹配', value: pattern ?? '' };
    case 'create_dir':
      return { label: '创建目录', value: dirPath ?? '' };
    case 'move_file':
      return {
        label: '移动文件',
        value: sourcePath && destPath
          ? `${extractFileNameFromPath(sourcePath)} → ${extractFileNameFromPath(destPath) ?? destPath}`
          : (sourcePath ?? destPath ?? ''),
      };
    default:
      return { label: getToolDisplayName(toolName), value: '' };
  }
}

/**
 * Compact-tool renderer for read-only / directory operations.
 *
 * Visual contract:
 *   - No card chrome: no border, no background fill, no padding box.
 *     The line sits inline within the assistant message's text flow.
 *   - While executing: text is dimmed and overlaid with a horizontal
 *     shimmer band (left-to-right sweep) so the user can tell it's
 *     in-flight at a glance.
 *   - Once done: text snaps to full opacity (shimmer stops), duration
 *     is shown on the right. Clicking anywhere on the line toggles an
 *     expanded view of the raw result below.
 *
 * Replaces the older `CompactToolCard` shell for the `COMPACT_TOOLS` set.
 * Other tools (write_file, edit_file, …) keep using `ToolCallCard`.
 */
export const InlineCompactTool: React.FC<InlineCompactToolProps> = React.memo(
  function InlineCompactTool({ item, trailingItems = [] }) {
    const [expanded, setExpanded] = useState(false);

    // Find the matching tool_result by toolCallId, if any. Stored items
    // arrive after the tool_call_start so we just walk forward.
    const result = useMemo(() => {
      for (const it of trailingItems) {
        if (it.type === 'tool_result' && it.toolCallId === item.toolCallId) {
          return it;
        }
      }
      return null;
    }, [trailingItems, item.toolCallId]);

    // `executing` while the start item is open AND no result has landed yet.
    // tool_result events set isExecuting to false via patchOutputItem.
    const executing = !result && item.isExecuting !== false;
    const hasError = result?.status === 'error';
    const duration = result?.duration ?? item.duration;

    const { label, value } = useMemo(
      () => buildHeadline(item.toolName, item.arguments ?? {}),
      [item.toolName, item.arguments],
    );

    const resultText = result?.result ?? '';
    // For errors, prefix the headline so the user sees something went wrong
    // even before expanding.
    const showErrorLabel = hasError;

    return (
      <div className={styles.wrapper}>
        <button
          type="button"
          className={`${styles.line} ${
            hasError ? styles.lineError : executing ? styles.lineExecuting : styles.lineDone
          }`}
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
          data-tool-call-id={item.toolCallId}
        >
          <span className={styles.caret} aria-hidden>
            {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          </span>

          <span className={styles.label}>
            {showErrorLabel ? `${label} 失败` : label}
            {value && <span className={styles.colon}>:</span>}
          </span>

          {value && (
            <span className={styles.value} data-shimmer={executing ? 'on' : 'off'}>
              {value}
            </span>
          )}

          <span className={styles.tail}>
            {executing && <span className={styles.executingHint}>执行中</span>}
            {!executing && duration !== undefined && (
              <span className={styles.duration}>{duration}ms</span>
            )}
          </span>
        </button>

        {expanded && resultText.length > 0 && (
          <pre className={styles.expanded}>
            {resultText}
          </pre>
        )}
        {expanded && !resultText && executing && (
          <div className={styles.expandedMuted}>工具正在执行…</div>
        )}
      </div>
    );
  },
);

export default InlineCompactTool;

/**
 * Predicate: does this `OutputItem` (a tool_call_start) belong to a
 * compact (inline) tool? Mirrors `COMPACT_TOOLS` in the registry so
 * callers don't have to import the set directly.
 */
export function isCompactToolItem(item: OutputItem): boolean {
  return item.type === 'tool_call_start' && COMPACT_TOOLS.has(item.toolName);
}
