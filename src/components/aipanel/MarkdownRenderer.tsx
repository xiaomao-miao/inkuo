import React, { useEffect, useRef } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import styles from './MarkdownRenderer.module.css';
import { openKnowledgeReference } from './knowledgeReference';

interface MarkdownRendererProps {
  content: string;
  className?: string;
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
  // href format: URL-encoded path + optional fragment #startLine,endLine
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

  // Decode the URL-encoded file path back to a real filesystem path
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

  // Handle legacy inkuo:// protocol links
  if (isKnowledgeReferenceHref(href)) {
    if (event.cancelable) event.preventDefault();
    event.stopPropagation();
    const { filePath, startLine, endLine } = parseKnowledgeHref(href);
    if (filePath) openKnowledgeReference({ filePath, documentTitle: '', startLine, endLine });
    return;
  }

  // Handle file-path links (href = absolute file path, hash = #startLine,endLine)
  // Skip external links and anchors
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

  // This looks like a file path link — intercept it
  if (event.cancelable) event.preventDefault();
  event.stopPropagation();

  const { filePath, startLine, endLine } = parseFragmentAndPath(href);
  if (filePath) openKnowledgeReference({ filePath, documentTitle: '', startLine, endLine });
}

export const MarkdownRenderer: React.FC<MarkdownRendererProps> = ({ content, className }) => {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    container.addEventListener('click', handleClick, true);
    return () => container.removeEventListener('click', handleClick, true);
  }, []);

  return (
    <div ref={containerRef} className={`${styles.markdown} ${className || ''}`}>
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
        {content}
      </ReactMarkdown>
    </div>
  );
};
