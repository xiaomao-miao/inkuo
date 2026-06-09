import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Sparkles, X, ChevronDown, Loader2 } from 'lucide-react';
import { useCmdKStore, useEditorStore, useSidebarStore } from '../../store';
import type { AIEditResponse, DiffResult } from '../../types';
import { getModifierKeyLabel } from '../../utils/platform';
import styles from './CmdK.module.css';

const SCOPE_OPTIONS = [
  { value: 'selection', label: '选区', description: '仅编辑选中的文本' },
  { value: 'paragraph', label: '段落', description: '编辑光标所在的段落' },
  { value: 'section', label: '章节', description: '编辑当前标题下的内容' },
  { value: 'document', label: '文档', description: '编辑整个文档' },
] as const;

const TEMPLATES = [
  { label: '更专业', instruction: '将语言改得更专业正式' },
  { label: '更精炼', instruction: '精简内容，保留核心信息' },
  { label: '润色语法', instruction: '修正语法错误，优化句式' },
  { label: '添加小标题', instruction: '为每个段落添加简洁的小标题' },
];

export const CmdK = () => {
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const {
    isOpen,
    scope,
    instruction,
    isProcessing,
    close,
    setScope,
    setInstruction,
    setIsProcessing,
    reset,
  } = useCmdKStore();
  
  const { selectedFile } = useSidebarStore();
  const { documentContents, setDiffHunks } = useEditorStore();
  
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const currentContent = currentDoc?.content || '';
  const selection = currentDoc?.selection || null;
  
  const [showScopeDropdown, setShowScopeDropdown] = useState(false);
  const modifierKey = getModifierKeyLabel();

  // Focus input when opened
  useEffect(() => {
    if (isOpen) {
      inputRef.current?.focus();
    }
  }, [isOpen]);

  // Close on Escape
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && isOpen) {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, close]);

  // Get selected or paragraph text
  const getTargetText = (): string => {
    if (selection && selection.from !== selection.to) {
      return currentContent.slice(selection.from, selection.to);
    }
    // Fallback: get first paragraph or first 500 chars
    const lines = currentContent.split('\n\n');
    return lines[0]?.slice(0, 500) || currentContent.slice(0, 500);
  };

  const handleSubmit = async () => {
    if (!instruction.trim() || isProcessing || !selectedFile) return;

    setIsProcessing(true);
    
    try {
      const response = await invoke<AIEditResponse>('ai_edit', {
        instruction: instruction.trim(),
        originalText: targetText,
        scope: scope,
        context: [],
      });

      // Compute diff
      const diffResult = await invoke<DiffResult>('compute_diff', {
        oldText: targetText,
        newText: response.content,
      });

      // Set diff hunks with context about where the diff applies in the full document
      const originalOffset = selection && selection.from !== selection.to
        ? selection.from
        : currentContent.indexOf(targetText);
      setDiffHunks(selectedFile, diffResult.hunks, targetText, originalOffset >= 0 ? originalOffset : 0);
      
      // Close modal
      close();
      reset();
    } catch (err) {
      console.error('AI edit failed:', err);
      alert(`AI 编辑失败: ${err}`);
    } finally {
      setIsProcessing(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleSubmit();
    }
  };

  if (!isOpen) return null;

  const selectedScope = SCOPE_OPTIONS.find(s => s.value === scope);
  const targetText = getTargetText();

  return (
    <div className={styles.overlay} onClick={close}>
      <div className={styles.modal} onClick={e => e.stopPropagation()}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <Sparkles size={18} className={styles.icon} />
            <span className={styles.title}>AI 编辑</span>
          </div>
          <button className={styles.closeButton} onClick={close}>
            <X size={16} />
          </button>
        </div>
        
        <div className={styles.scopeBar}>
          <div className={styles.scopeSelector}>
            <button 
              className={styles.scopeButton}
              onClick={() => setShowScopeDropdown(!showScopeDropdown)}
            >
              <span>{selectedScope?.label}</span>
              <ChevronDown size={14} />
            </button>
            
            {showScopeDropdown && (
              <div className={styles.scopeDropdown}>
                {SCOPE_OPTIONS.map(option => (
                  <div
                    key={option.value}
                    className={`${styles.scopeOption} ${scope === option.value ? styles.selected : ''}`}
                    onClick={() => {
                      setScope(option.value);
                      setShowScopeDropdown(false);
                    }}
                  >
                    <span className={styles.optionLabel}>{option.label}</span>
                    <span className={styles.optionDesc}>{option.description}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
          
          <div className={styles.templates}>
            {TEMPLATES.map(template => (
              <button
                key={template.label}
                className={styles.templateButton}
                onClick={() => setInstruction(template.instruction)}
              >
                {template.label}
              </button>
            ))}
          </div>
        </div>
        
        <div className={styles.inputArea}>
          <textarea
            ref={inputRef}
            className={styles.input}
            placeholder="输入你的编辑指令..."
            value={instruction}
            onChange={e => setInstruction(e.target.value)}
            onKeyDown={handleKeyDown}
            rows={3}
          />
        </div>
        
        <div className={styles.preview}>
          <span className={styles.previewLabel}>将应用于:</span>
          <code className={styles.previewText}>
            {targetText.slice(0, 100)}
            {targetText.length > 100 && '...'}
          </code>
        </div>
        
        <div className={styles.footer}>
          <span className={styles.hint}>
            <kbd>{modifierKey}</kbd> + <kbd>Enter</kbd> 发送
          </span>
          <button 
            className={styles.submitButton}
            onClick={handleSubmit}
            disabled={!instruction.trim() || isProcessing}
          >
            {isProcessing ? (
              <>
                <Loader2 size={14} className={styles.spinner} />
                <span>处理中...</span>
              </>
            ) : (
              <>
                <Sparkles size={14} />
                <span>开始编辑</span>
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
};
