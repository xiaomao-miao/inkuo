import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertCircle, Sparkles, X, ChevronDown, Loader2 } from 'lucide-react';
import { useCmdKStore, useEditorStore, useSidebarStore } from '../../store';
import type { AIEditResponse, DiffResult, EditScope } from '../../types';
import { reportError } from '../../utils/errors';
import { getModifierKeyLabel } from '../../utils/platform';
import { Tooltip } from '../common/Tooltip';
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

const SCOPE_TO_REQUEST: Record<(typeof SCOPE_OPTIONS)[number]['value'], EditScope> = {
  selection: 'Selection',
  paragraph: 'Paragraph',
  section: 'Section',
  document: 'Document',
};

export const CmdK = () => {
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const isOpen = useCmdKStore((state) => state.isOpen);
  const scope = useCmdKStore((state) => state.scope);
  const instruction = useCmdKStore((state) => state.instruction);
  const isProcessing = useCmdKStore((state) => state.isProcessing);
  const close = useCmdKStore((state) => state.close);
  const setScope = useCmdKStore((state) => state.setScope);
  const setInstruction = useCmdKStore((state) => state.setInstruction);
  const setIsProcessing = useCmdKStore((state) => state.setIsProcessing);
  const reset = useCmdKStore((state) => state.reset);
  
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const documentContents = useEditorStore((state) => state.documentContents);
  const setDiffHunks = useEditorStore((state) => state.setDiffHunks);
  
  const currentDoc = selectedFile ? documentContents[selectedFile] : null;
  const currentMetadata = currentDoc?.metadata;
  const currentContent = currentMetadata?.content ?? '';
  const selection = currentMetadata?.selection ?? null;
  
  const [showScopeDropdown, setShowScopeDropdown] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
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

  useEffect(() => {
    if (!isOpen && errorMessage) {
      setErrorMessage(null);
    }
  }, [isOpen, errorMessage]);

  // Get target text based on selected scope
  const getTargetText = useMemo(() => {
    return (targetScope: (typeof SCOPE_OPTIONS)[number]['value']): string => {
      if (!currentContent) return '';

      if (targetScope === 'selection') {
        if (selection && selection.from !== selection.to) {
          return currentContent.slice(selection.from, selection.to);
        }
        return currentContent.slice(0, 500);
      }

      if (targetScope === 'document') {
        return currentContent;
      }

      const lines = currentContent.split('\n');
      const lineStarts: number[] = [];
      let offset = 0;

      for (const line of lines) {
        lineStarts.push(offset);
        offset += line.length + 1;
      }

      const clampOffset = selection?.from ?? 0;
      const resolvedLineIndex = lineStarts.findIndex((start, index) => {
        const nextStart = index + 1 < lineStarts.length ? lineStarts[index + 1] : currentContent.length + 1;
        return clampOffset >= start && clampOffset < nextStart;
      });
      const currentLineIndex = resolvedLineIndex >= 0 ? resolvedLineIndex : 0;

      const getLineOffset = (lineIndex: number) => lineStarts[Math.max(0, Math.min(lineIndex, lineStarts.length - 1))] ?? 0;

      if (targetScope === 'paragraph') {
        let startLine = currentLineIndex;
        let endLine = currentLineIndex;

        while (startLine > 0 && lines[startLine - 1]?.trim() !== '') {
          startLine -= 1;
        }
        while (endLine < lines.length - 1 && lines[endLine + 1]?.trim() !== '') {
          endLine += 1;
        }

        const startOffset = getLineOffset(startLine);
        const endOffset = endLine + 1 < lineStarts.length ? getLineOffset(endLine + 1) - 1 : currentContent.length;
        return currentContent.slice(startOffset, endOffset).trim();
      }

      let sectionStartLine = 0;
      for (let index = currentLineIndex; index >= 0; index -= 1) {
        if (/^#{1,6}\s/.test(lines[index] ?? '')) {
          sectionStartLine = index;
          break;
        }
      }

      let sectionEndLine = lines.length;
      for (let index = currentLineIndex + 1; index < lines.length; index += 1) {
        if (/^#{1,6}\s/.test(lines[index] ?? '')) {
          sectionEndLine = index;
          break;
        }
      }

      const sectionStartOffset = getLineOffset(sectionStartLine);
      const sectionEndOffset = sectionEndLine < lineStarts.length ? getLineOffset(sectionEndLine) - 1 : currentContent.length;
      return currentContent.slice(sectionStartOffset, sectionEndOffset).trim();
    };
  }, [currentContent, selection]);

  const targetText = useMemo(() => getTargetText(scope), [getTargetText, scope]);

  const handleSubmit = async () => {
    if (!instruction.trim() || isProcessing || !selectedFile || !targetText.trim()) return;

    setIsProcessing(true);
    setErrorMessage(null);
    
    try {
      const response = await invoke<AIEditResponse>('ai_edit', {
        instruction: instruction.trim(),
        originalText: targetText,
        scope: SCOPE_TO_REQUEST[scope],
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
      setErrorMessage(reportError('cmdk-ai-edit', err));
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

  return (
    <div className={styles.overlay} onClick={close}>
      <div className={styles.modal} onClick={e => e.stopPropagation()}>
        <div className={styles.header}>
          <div className={styles.headerLeft}>
            <Sparkles size={18} className={styles.icon} />
            <span className={styles.title}>AI 编辑</span>
          </div>
          <Tooltip content="关闭" side="left" shortcut="Esc">
            <button className={styles.closeButton} onClick={close}>
              <X size={16} />
            </button>
          </Tooltip>
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

        {errorMessage && (
          <div className={styles.errorMessage} role="alert">
            <AlertCircle size={14} />
            <span>AI 编辑失败：{errorMessage}</span>
          </div>
        )}
        
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
