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

function findValidMarkdownPrefix(text: string): string {
  if (!text.includes('```') && !text.includes('\n#')) {
    return text;
  }

  const codeBlockPattern = /```[\w]*\n[\s\S]*?$/;
  const match = text.match(codeBlockPattern);
  if (match) {
    const codeBlockStart = match[0].indexOf('\n') + 1;
    if (codeBlockStart > 0 && codeBlockStart < match[0].length) {
      return text.slice(0, match.index! + codeBlockStart);
    }
  }

  const lines = text.split('\n');
  let validUpTo = text.length;

  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i];
    if (line.trim() === '') continue;

    if (line.startsWith('#')) {
      const hashCount = line.match(/^#+/)?.[0].length || 0;
      if (line.slice(hashCount).trim() === '') {
        validUpTo = text.indexOf(line);
        continue;
      }
    }

    if (line.match(/^```/)) {
      const prevLine = i > 0 ? lines[i - 1] : '';
      if (!prevLine.match(/```.*$/)) {
        validUpTo = text.indexOf(line);
        continue;
      }
    }

    break;
  }

  return text.slice(0, validUpTo);
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
          code({ node, className: codeClassName, children, ...props }) {
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
