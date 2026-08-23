import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import { streamAgent } from './agent';

describe('streamAgent', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('forwards real image attachments instead of replacing them with an empty list', async () => {
    const imageAttachments = [{
      path: '/workspace/previews/page-001.png',
      detail: 'high' as const,
      name: 'page-001.png',
    }];

    await streamAgent({
      sessionId: 'session-1',
      messageId: 'message-1',
      instruction: '检查这一页的排版',
      workspacePath: '/workspace',
      imageAttachments,
      configInput: {
        provider: 'openai',
        api_key: 'test',
        base_url: 'https://example.invalid',
        model: 'vision-model',
      },
    });

    expect(invokeMock).toHaveBeenCalledWith('ai_agent_stream', expect.objectContaining({
      imageAttachments,
    }));
  });

  it('uses an empty attachment list only when the caller supplies none', async () => {
    await streamAgent({
      sessionId: 'session-1',
      messageId: 'message-1',
      instruction: 'hello',
      configInput: {
        provider: 'ollama',
        api_key: null,
        base_url: 'http://localhost:11434',
        model: 'text-model',
      },
    });

    expect(invokeMock).toHaveBeenCalledWith('ai_agent_stream', expect.objectContaining({
      imageAttachments: [],
    }));
  });
});
