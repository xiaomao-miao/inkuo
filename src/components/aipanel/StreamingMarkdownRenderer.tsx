import React, { useMemo, useEffect } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import styles from './MarkdownRenderer.module.css';
import { openKnowledgeReference } from './knowledgeReference';

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

const KNOWLEDGE_PREFIX = 'inkuo://knowledge-reference?';

function isKnowledgeReferenceHref(href: string | undefined): boolean {
  return !!href && href.startsWith(KNOWLEDGE_PREFIX);
}

function parseKnowledgeHref(href: string): { filePath: string; startLine?: number; endLine?: number } {
  try {
    const url = new URL(href);
    return {
      filePath: url.searchParams.get('path') || '',
      startLine: Number(url.searchParams.get('startLine')) || undefined,
      endLine: Number(url.searchParams.get('endLine')) || undefined,
    };
  } catch {
    return { filePath: '' };
  }
}

function parseFragmentAndPath(href: string): { filePath: string; startLine?: number; endLine?: number } {
  const hashIndex = href.indexOf('#');
  const encodedPath = hashIndex >= 0 ? href.slice(0, hashIndex) : href;
  const fragment = hashIndex >= 0 ? href.slice(hashIndex + 1) : '';

  let startLine: number | undefined;
  let endLine: number | undefined;

  if (fragment) {
    const parts = fragment.split(',');
    const s = Number(parts[0]);
    startLine = isNaN(s) ? undefined : s;
    if (parts.length >= 2) {
      const e = Number(parts[1]);
      endLine = isNaN(e) ? undefined : e;
    }
  }

  let filePath: string;
  try {
    filePath = decodeURIComponent(encodedPath);
  } catch {
    filePath = encodedPath;
  }

  return { filePath, startLine, endLine };
}

function handleClick(event: MouseEvent) {
  const target = event.target as HTMLElement;
  const anchor = target.closest ? target.closest('a') : null;
  if (!anchor) return;

  const href = anchor.getAttribute('href');
  if (!href) return;

  if (isKnowledgeReferenceHref(href)) {
    if (event.cancelable) event.preventDefault();
    event.stopPropagation();
    const { filePath, startLine, endLine } = parseKnowledgeHref(href);
    if (filePath) openKnowledgeReference({ filePath, documentTitle: '', startLine, endLine });
    return;
  }

  if (
    href.startsWith('http://') ||
    href.startsWith('https://') ||
    href.startsWith('mailto:') ||
    href.startsWith('tel:') ||
    href.startsWith('#') ||
    !href.startsWith('/')
  ) {
    return;
  }

  if (event.cancelable) event.preventDefault();
  event.stopPropagation();

  const { filePath, startLine, endLine } = parseFragmentAndPath(href);
  if (filePath) openKnowledgeReference({ filePath, documentTitle: '', startLine, endLine });
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

  useEffect(() => {
    const container = document; // attach globally to catch streaming content too
    container.addEventListener('click', handleClick, true);
    return () => container.removeEventListener('click', handleClick, true);
  }, []);

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
