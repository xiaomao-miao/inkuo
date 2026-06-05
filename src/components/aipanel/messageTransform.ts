import type { ChatMessage, SearchResult } from '../../store';

export function normalizeSearchResults(results: SearchResult[] | undefined): SearchResult[] | undefined {
  if (!results?.length) return undefined;

  return results
    .filter((result): result is SearchResult & { filePath: string } => {
      return !!result && typeof result.filePath === 'string' && result.filePath.trim().length > 0;
    })
    .map((result) => ({
      ...result,
      documentTitle:
        typeof result.documentTitle === 'string' && result.documentTitle.trim().length > 0
          ? result.documentTitle
          : result.filePath.split('/').pop() || '未命名文档',
      score: Number.isFinite(result.score) ? result.score : 0,
    }));
}

export function buildConversationHistory(messages: ChatMessage[]) {
  return messages.map((message) => {
    let textContent = '';

    if (message.role === 'tool') {
      textContent = message.content || '';
    } else if (message.outputItems && message.outputItems.length > 0) {
      textContent = message.outputItems
        .filter((item) => item.type === 'text')
        .map((item) => item.content)
        .join('');
    } else {
      textContent = message.content || '';
    }

    return {
      id: message.id,
      role: message.role,
      content: textContent,
      tool_calls: message.toolCalls,
      tool_call_id: message.toolCallId,
    };
  });
}
