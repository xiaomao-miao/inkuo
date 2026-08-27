import type { OutputItem } from '../../types';
import {
  formatArgumentsForDisplay,
  getExpertDisplayName,
  getToolDisplayName,
} from './toolUtils';

export type MinimalActivityStatus = 'working' | 'success' | 'error';

export interface MinimalActivity {
  key: string;
  label: string;
  detail?: string;
  status: MinimalActivityStatus;
}

export function shouldRenderOutputItemInMinimal(item: OutputItem): boolean {
  return item.type === 'text' || (
    item.type === 'tool_call_start' && item.toolName === 'ask_user'
  );
}

function firstUsefulArgument(
  toolName: string,
  args: Record<string, unknown>,
  rawArguments?: string,
): string | undefined {
  const parsedArgs = Object.keys(args).length > 0 ? args : null;
  const formatted = formatArgumentsForDisplay(toolName, parsedArgs, rawArguments);
  const firstLine = formatted?.split('\n').find((line) => line.trim().length > 0)?.trim();
  return firstLine || undefined;
}

function compactReasoningDetail(content: string): string | undefined {
  const lines = content
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  const lastLine = lines[lines.length - 1];
  if (!lastLine) return undefined;
  const compact = lastLine.replace(/\s+/g, ' ');
  return compact.length > 120 ? `${compact.slice(0, 117)}…` : compact;
}

export function buildMinimalActivities(outputItems: OutputItem[]): MinimalActivity[] {
  const activities: MinimalActivity[] = [];
  for (const item of outputItems) {
    if (item.type === 'reasoning') {
      activities.push({
        key: item.reasoningId ?? `reasoning-${activities.length}`,
        label: item.completed ? '分析完成' : '正在分析任务',
        detail: compactReasoningDetail(item.content),
        status: item.completed ? 'success' : 'working',
      });
      continue;
    }
    if (item.type === 'tool_call_start' && item.toolName !== 'ask_user') {
      const isDelegate = item.toolName === 'delegate_to';
      const expert = (item.arguments.expert as string) || '';
      const task = (item.arguments.task as string) || '';
      activities.push({
        key: item.toolCallId,
        label: isDelegate
          ? `委派给 ${getExpertDisplayName(expert)}`
          : getToolDisplayName(item.toolName),
        detail: isDelegate
          ? task || undefined
          : firstUsefulArgument(item.toolName, item.arguments, item.rawArguments),
        status: item.status === 'error'
          ? 'error'
          : item.status === 'success'
            ? 'success'
            : 'working',
      });
      continue;
    }
    if (item.type === 'tool_error') {
      activities.push({
        key: `error-${item.toolCallId}`,
        label: '工具执行失败',
        detail: item.error,
        status: 'error',
      });
    }
  }
  return activities.slice(-3);
}
