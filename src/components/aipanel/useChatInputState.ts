import { useCallback, useState } from 'react';
import type { ImageAttachmentInput } from '../../types';

export function useChatInputState() {
  const [input, setInput] = useState('');
  const [editingMessageId, setEditingMessageId] = useState<string | null>(null);
  const [editingContent, setEditingContent] = useState('');
  const [imageAttachments, setImageAttachments] = useState<ImageAttachmentInput[]>([]);

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
    setImageAttachments([]);
  }, [clearEditingState]);

  return {
    input,
    setInput,
    editingMessageId,
    editingContent,
    setEditingContent,
    imageAttachments,
    setImageAttachments,
    clearEditingState,
    startEdit,
    cancelEdit,
  };
}
