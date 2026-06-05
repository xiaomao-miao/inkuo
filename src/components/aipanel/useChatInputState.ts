import { useCallback, useState } from 'react';

export function useChatInputState() {
  const [input, setInput] = useState('');
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');

  const clearEditingState = useCallback(() => {
    setEditingMessageId(null);
    setEditingContent('');
  }, []);

  const startEdit = useCallback((messageId: string, currentContent: string) => {
    setEditingMessageId(messageId);
    setEditingContent(currentContent);
    setInput(currentContent);
  }, []);

  const cancelEdit = useCallback(() => {
    clearEditingState();
    setInput('');
  }, [clearEditingState]);

  return {
    input,
    setInput,
    editingMessageId,
    editingContent,
    setEditingContent,
    clearEditingState,
    startEdit,
    cancelEdit,
  };
}
