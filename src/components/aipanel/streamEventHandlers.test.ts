import { afterEach, describe, expect, it, vi } from 'vitest';
import { useEditorStore, useSidebarStore } from '../../store';
import { handleToolResult } from './streamEventHandlers';

const originalSidebarState = useSidebarStore.getState();
const originalEditorState = useEditorStore.getState();

afterEach(() => {
  useSidebarStore.setState(originalSidebarState, true);
  useEditorStore.setState(originalEditorState, true);
});

describe('Office modification stream events', () => {
  it('invalidates the disk buffer without clearing a dirty open tab', () => {
    const absolutePath = '/workspace/paper.docx';
    useSidebarStore.setState({
      workspacePath: '/workspace',
      openTabs: [{
        id: 'paper',
        path: absolutePath,
        name: 'paper.docx',
        isDirty: true,
      }],
      activeTabId: 'paper',
      selectedFile: absolutePath,
    });
    useEditorStore.setState({ documentContents: {} });

    handleToolResult({
      session_id: 'session',
      message_id: 'message',
      event_type: 'tool_result',
      tool_call_id: 'tool',
      content: 'ok',
      done: false,
      office_file_modified: {
        path: 'paper.docx',
        format: 'docx',
      },
    }, vi.fn());

    expect(useSidebarStore.getState().openTabs[0]?.isDirty).toBe(true);
    expect(
      useEditorStore.getState().documentContents[absolutePath]?.office.bufferVersion,
    ).toBe(1);
  });
});
