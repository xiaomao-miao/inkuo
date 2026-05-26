import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { 
  MessageSquare, 
  Wand2, 
  PanelRightClose, 
  Loader2,
  Copy,
  ArrowUp,
  Trash2
} from 'lucide-react';
import { useAIPanelStore, type ChatMessage } from '../../store';
import styles from './AIPanel.module.css';

export const AIPanel: React.FC = () => {
  const {
    activeTab,
    messages,
    isStreaming,
    setActiveTab,
    addMessage,
    setIsStreaming,
    clearMessages,
  } = useAIPanelStore();
  
  const [input, setInput] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  // Scroll to bottom when new messages arrive
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  // Auto-resize textarea
  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.style.height = 'auto';
      inputRef.current.style.height = `${Math.min(inputRef.current.scrollHeight, 120)}px`;
    }
  }, [input]);

  const handleSend = async () => {
    if (!input.trim() || isStreaming) return;

    const userMessage: ChatMessage = {
      id: Date.now().toString(),
      role: 'user',
      content: input.trim(),
      timestamp: Date.now(),
    };
    
    addMessage(userMessage);
    setInput('');
    setIsStreaming(true);

    try {
      // For now, echo back with a simulated response
      // In production, this would call the AI provider
      const response = await invoke<string>('ai_edit', {
        instruction: input.trim(),
        originalText: 'Hello, this is a test document.',
        scope: 'selection',
        context: [],
      });

      const assistantMessage: ChatMessage = {
        id: (Date.now() + 1).toString(),
        role: 'assistant',
        content: `已完成您的请求。\n\n修改内容：\n${response}`,
        timestamp: Date.now(),
      };
      
      addMessage(assistantMessage);
    } catch (err) {
      const errorMessage: ChatMessage = {
        id: (Date.now() + 1).toString(),
        role: 'assistant',
        content: `抱歉，发生了错误：${err}`,
        timestamp: Date.now(),
      };
      addMessage(errorMessage);
    } finally {
      setIsStreaming(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleCopy = (content: string) => {
    navigator.clipboard.writeText(content);
  };

  return (
    <aside className={styles.panel}>
      <div className={styles.header}>
        <div className={styles.tabs}>
          <button
            className={`${styles.tab} ${activeTab === 'chat' ? styles.active : ''}`}
            onClick={() => setActiveTab('chat')}
          >
            <MessageSquare size={14} />
            <span>对话</span>
          </button>
          <button
            className={`${styles.tab} ${activeTab === 'edit' ? styles.active : ''}`}
            onClick={() => setActiveTab('edit')}
          >
            <Wand2 size={14} />
            <span>编辑</span>
          </button>
        </div>
        <button className={styles.closeButton} title="关闭面板">
          <PanelRightClose size={16} />
        </button>
      </div>

      <div className={styles.content}>
        {messages.length === 0 ? (
          <div className={styles.emptyState}>
            <div className={styles.emptyIcon}>
              <MessageSquare size={32} />
            </div>
            <h3>开始对话</h3>
            <p>输入你的问题或指令</p>
          </div>
        ) : (
          <div className={styles.messages}>
            {messages.map(message => (
              <div 
                key={message.id}
                className={`${styles.message} ${styles[message.role]}`}
              >
                <div className={styles.messageHeader}>
                  <span className={styles.messageRole}>
                    {message.role === 'user' ? '你' : 'AI'}
                  </span>
                  <div className={styles.messageActions}>
                    <button 
                      className={styles.actionBtn}
                      onClick={() => handleCopy(message.content)}
                      title="复制"
                    >
                      <Copy size={12} />
                    </button>
                  </div>
                </div>
                <div className={styles.messageContent}>
                  {message.content.split('\n').map((line, i) => (
                    <span key={i}>
                      {line}
                      {i < message.content.split('\n').length - 1 && <br />}
                    </span>
                  ))}
                </div>
              </div>
            ))}
            {isStreaming && (
              <div className={`${styles.message} ${styles.assistant}`}>
                <div className={styles.messageHeader}>
                  <span className={styles.messageRole}>AI</span>
                </div>
                <div className={styles.messageContent}>
                  <Loader2 size={14} className={styles.loadingSpinner} />
                  <span>正在思考...</span>
                </div>
              </div>
            )}
            <div ref={messagesEndRef} />
          </div>
        )}
      </div>

      <div className={styles.inputArea}>
        <textarea
          ref={inputRef}
          className={styles.input}
          placeholder="输入消息... (Shift+Enter 换行)"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          rows={1}
        />
        <div className={styles.inputActions}>
          {messages.length > 0 && (
            <button 
              className={styles.clearBtn}
              onClick={clearMessages}
              title="清空对话"
            >
              <Trash2 size={14} />
            </button>
          )}
          <button 
            className={styles.sendBtn}
            onClick={handleSend}
            disabled={!input.trim() || isStreaming}
          >
            {isStreaming ? (
              <Loader2 size={16} className={styles.loadingSpinner} />
            ) : (
              <ArrowUp size={16} />
            )}
          </button>
        </div>
      </div>
    </aside>
  );
};
