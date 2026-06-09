import React, { useMemo } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import styles from './MarkdownRenderer.module.css';

interface StreamingMarkdownRendererProps {
  content: string;
  className?: string;
  isStreaming?: boolean;
}

const MARKDOWN_INDICATORS = [
  /^#{1,6}\s/m,
  /\*\*[^*]+\*\*/m,
  /\*[^*]+\*/m,
  /^```/m,
  /`[^`]+`/m,
  /^\s*[-*+]\s/m,
  /^\s*\d+\.\s/m,
  /^>\s/m,
  /\[.+\]\(.+\)/m,
  /!\[.+\]\(.+\)/m,
  /^---$/m,
  /\|.+\|/m,
];

function looksLikeMarkdown(text: string): boolean {
  for (const pattern of MARKDOWN_INDICATORS) {
    if (pattern.test(text)) return true;
  }
  return false;
}

function findSafeMarkdownBoundary(text: string): number {
  let fenceCount = 0;
  let inlineCodeOpen = false;
  let boundary = text.length;

  for (let index = 0; index < text.length; index += 1) {
    if (text.startsWith('```', index)) {
      fenceCount += 1;
      index += 2;
      continue;
    }

    if (text[index] === '`') {
      inlineCodeOpen = !inlineCodeOpen;
      boundary = index;
    }
  }

  if (fenceCount % 2 !== 0) {
    return text.lastIndexOf('```');
  }

  if (inlineCodeOpen) {
    return boundary;
  }

  const lines = text.split('\n');
  let consumedLength = text.length;

  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index];
    if (!line.trim()) {
      consumedLength -= line.length + 1;
      continue;
    }

    if (/^#{1,6}\s*$/.test(line) || /^>\s*$/.test(line) || /^[-*+]\s*$/.test(line) || /^\d+\.\s*$/.test(line)) {
      consumedLength -= line.length + 1;
      continue;
    }

    break;
  }

  return Math.max(0, consumedLength);
}

function findValidMarkdownPrefix(text: string): string {
  if (!text.includes('`') && !text.includes('\n#') && !text.includes('\n-') && !text.includes('\n>')) {
    return text;
  }

  const boundary = findSafeMarkdownBoundary(text);
  return text.slice(0, boundary);
}

export const StreamingMarkdownRenderer: React.FC<StreamingMarkdownRendererProps> = ({
  content,
  className,
  isStreaming = false,
}) => {
  const { renderedContent, hasMore } = useMemo(() => {
    if (!content) return { renderedContent: '', hasMore: false };

    if (!isStreaming) {
      return { renderedContent: content, hasMore: false };
    }

    if (!looksLikeMarkdown(content)) {
      return { renderedContent: content, hasMore: false };
    }

    const safeContent = findValidMarkdownPrefix(content);
    return { renderedContent: safeContent, hasMore: safeContent.length < content.length };
  }, [content, isStreaming]);

  return (
    <div className={`${styles.markdown} ${className || ''}`}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          code({ className: codeClassName, children, ...props }) {
            const match = /language-(\w+)/.exec(codeClassName || '');
            const isInline = !match && !codeClassName;

            if (isInline) {
              return (
                <code className={styles.inlineCode} {...props}>
                  {children}
                </code>
              );
            }

            return (
              <pre className={styles.codeBlock}>
                <code className={match ? `language-${match[1]}` : ''} {...props}>
                  {children}
                </code>
              </pre>
            );
          },
          a({ href, children, ...props }) {
            const isExternal = href?.startsWith('http://') || href?.startsWith('https://');
            return (
              <a
                href={href}
                className={styles.link}
                target={isExternal ? '_blank' : undefined}
                rel={isExternal ? 'noopener noreferrer' : undefined}
                {...props}
              >
                {children}
              </a>
            );
          },
          table({ children, ...props }) {
            return (
              <div className={styles.tableWrapper}>
                <table className={styles.table} {...props}>
                  {children}
                </table>
              </div>
            );
          },
        }}
      >
        {renderedContent}
      </ReactMarkdown>
      {hasMore && <span className={styles.streamingCaret} />}
    </div>
  );
};
