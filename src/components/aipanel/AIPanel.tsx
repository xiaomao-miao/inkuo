import React, { useState, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { 
  MessageSquare, 
  Wand2, 
  PanelRightClose, 
  Loader2,
  Copy,
  Trash2,
  Check,
  X,
  ChevronDown,
  FileText,
  MousePointer,
  Sparkles,
  Plus,
  Minus,
  AlignLeft,
  FileCode,
  Send
} from 'lucide-react';
import { useAIPanelStore, useEditorStore, useSidebarStore, type ChatMessage } from '../../store';
import styles from './AIPanel.module.css';

type ScopeType = 'selection' | 'paragraph' | 'section' | 'document';

// Template prompts
const TEMPLATES = [
  { icon: <Sparkles size={14} />, label: '更专业', prompt: '请用更专业的语气重写' },
  { icon: <Minus size={14} />, label: '更精炼', prompt: '请更精炼地表达，去除冗余' },
  { icon: <AlignLeft size={14} />, label: '改写成表格', prompt: '请改写成表格格式' },
  { icon: <FileCode size={14} />, label: '生成小标题', prompt: '请生成合适的小标题' },
  { icon: <Plus size={14} />, label: '扩展内容', prompt: '请扩展这段内容，提供更多细节' },
  { icon: <FileText size={14} />, label: '翻译英文', prompt: '请翻译为英文并保留专业术语' },
];

export const AIPanel: React.FC = () => {
  const {
    activeTab,
    messages,
    isStreaming,
    currentDiff,
    setActiveTab,
    addMessage,
    setIsStreaming,
    clearMessages,
    setCurrentDiff,
    acceptAllHunks,
    rejectAllHunks,
  } = useAIPanelStore();
  
  const { selectedFile } = useSidebarStore();
  const { documentContents, getSelection } = useEditorStore();
  
  const [input, setInput] = useState('');
  const [scope, setScope] = useState<ScopeType>('selection');
  const [showScopeMenu, setShowScopeMenu] = useState(false);
  const [showTemplates, setShowTemplates] = useState(false);
  
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
      inputRef.current.style.height = `${Math.min(inputRef.current.scrollHeight, 150)}px`;
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
      // Get current editor content and selection
      const selection = getSelection();
      const currentDoc = selectedFile ? documentContents[selectedFile] : null;
      const originalText = selection || currentDoc?.content || '';

      // Call AI edit
      const response = await invoke<any>('ai_edit', {
        instruction: input.trim(),
        originalText: originalText.slice(0, 5000),
        scope: scope,
        context: [],
      });

      const assistantMessage: ChatMessage = {
        id: (Date.now() + 1).toString(),
        role: 'assistant',
        content: response.content || response,
        timestamp: Date.now(),
      };
      
      addMessage(assistantMessage);
      
      // If there's a diff, store it
      if (response.diff) {
        setCurrentDiff({
          originalText,
          newText: response.content,
          hunks: response.diff.hunks || [],
          summary: response.summary || 'AI 已修改内容',
        });
      }
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

  const handleTemplateClick = (prompt: string) => {
    setInput(prev => prev ? prev + ' ' + prompt : prompt);
    setShowTemplates(false);
    inputRef.current?.focus();
  };

  const getScopeLabel = () => {
    switch (scope) {
      case 'selection': return '选区';
      case 'paragraph': return '段落';
      case 'section': return '章节';
      case 'document': return '文档';
      default: return '选区';
    }
  };

  const getScopeIcon = () => {
    switch (scope) {
      case 'selection': return <MousePointer size={14} />;
      case 'paragraph': return <AlignLeft size={14} />;
      case 'section': return <FileText size={14} />;
      case 'document': return <FileCode size={14} />;
      default: return <MousePointer size={14} />;
    }
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
        {activeTab === 'chat' ? (
          <>
            {messages.length === 0 ? (
              <div className={styles.emptyState}>
                <div className={styles.emptyIcon}>
                  <Sparkles size={32} />
                </div>
                <h3>开始对话</h3>
                <p>询问关于文档的问题或请求 AI 帮助你写作</p>
                <div className={styles.quickActions}>
                  <button 
                    className={styles.quickAction}
                    onClick={() => setInput('总结这篇文档的主要内容')}
                  >
                    总结文档
                  </button>
                  <button 
                    className={styles.quickAction}
                    onClick={() => setInput('解释这段代码/文本的工作原理')}
                  >
                    解释内容
                  </button>
                </div>
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
                      {message.content}
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
          </>
        ) : (
          <>
            {/* Edit Tab */}
            <div className={styles.editHeader}>
              <div className={styles.scopeSelector}>
                <button 
                  className={styles.scopeButton}
                  onClick={() => setShowScopeMenu(!showScopeMenu)}
                >
                  {getScopeIcon()}
                  <span>{getScopeLabel()}</span>
                  <ChevronDown size={12} />
                </button>
                {showScopeMenu && (
                  <div className={styles.scopeMenu}>
                    {(['selection', 'paragraph', 'section', 'document'] as ScopeType[]).map(s => (
                      <button
                        key={s}
                        className={`${styles.scopeMenuItem} ${scope === s ? styles.active : ''}`}
                        onClick={() => { setScope(s); setShowScopeMenu(false); }}
                      >
                        {s === 'selection' && <MousePointer size={14} />}
                        {s === 'paragraph' && <AlignLeft size={14} />}
                        {s === 'section' && <FileText size={14} />}
                        {s === 'document' && <FileCode size={14} />}
                        <span>
                          {s === 'selection' && '选区'}
                          {s === 'paragraph' && '段落'}
                          {s === 'section' && '章节'}
                          {s === 'document' && '文档'}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <button 
                className={styles.templateButton}
                onClick={() => setShowTemplates(!showTemplates)}
              >
                <Sparkles size={14} />
                <span>模板</span>
              </button>
            </div>

            {showTemplates && (
              <div className={styles.templates}>
                {TEMPLATES.map((t, i) => (
                  <button
                    key={i}
                    className={styles.templateItem}
                    onClick={() => handleTemplateClick(t.prompt)}
                  >
                    {t.icon}
                    <span>{t.label}</span>
                  </button>
                ))}
              </div>
            )}

            {currentDiff ? (
              <div className={styles.diffContainer}>
                <div className={styles.diffHeader}>
                  <h4>AI 修改建议</h4>
                  <div className={styles.diffActions}>
                    <button 
                      className={styles.diffBtnAccept}
                      onClick={acceptAllHunks}
                    >
                      <Check size={14} />
                      接受全部
                    </button>
                    <button 
                      className={styles.diffBtnReject}
                      onClick={rejectAllHunks}
                    >
                      <X size={14} />
                      拒绝全部
                    </button>
                  </div>
                </div>
                <div className={styles.diffSummary}>
                  {currentDiff.summary}
                </div>
                <div className={styles.diffContent}>
                  <div className={styles.diffOld}>
                    <div className={styles.diffLabel}>
                      <Minus size={12} />
                      <span>原文</span>
                    </div>
                    <pre className={styles.diffText}>{currentDiff.originalText}</pre>
                  </div>
                  <div className={styles.diffNew}>
                    <div className={styles.diffLabel}>
                      <Plus size={12} />
                      <span>修改后</span>
                    </div>
                    <pre className={styles.diffText}>{currentDiff.newText}</pre>
                  </div>
                </div>
              </div>
            ) : (
              <div className={styles.editEmpty}>
                <Wand2 size={32} />
                <h3>AI 编辑</h3>
                <p>输入指令，AI 将帮你修改文档内容</p>
                <div className={styles.editTips}>
                  <div className={styles.tip}>
                    <kbd>Tab</kbd> 接受修改
                  </div>
                  <div className={styles.tip}>
                    <kbd>Esc</kbd> 拒绝修改
                  </div>
                </div>
              </div>
            )}
          </>
        )}
      </div>

      {/* Input Area */}
      <div className={styles.inputArea}>
        <textarea
          ref={inputRef}
          className={styles.input}
          placeholder={
            activeTab === 'chat' 
              ? '输入消息... (Enter 发送，Shift+Enter 换行)' 
              : '输入编辑指令... (例如：让这段话更专业)'
          }
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          rows={1}
        />
        <div className={styles.inputActions}>
          {messages.length > 0 && (
            <button 
              className={styles.iconBtn}
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
              <Send size={16} />
            )}
          </button>
        </div>
      </div>
    </aside>
  );
};
