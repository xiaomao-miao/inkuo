import React, { useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import { Check, Copy } from 'lucide-react';
import styles from './MarkdownRenderer.module.css';

interface MarkdownRendererProps {
  content: string;
  className?: string;
  /** Callback when user clicks on a file path */
  onFileClick?: (filePath: string) => void;
  /** Current workspace root path for resolving relative file paths */
  workspacePath?: string;
}

/**
 * Preprocess content to convert <file> tags to Markdown links.
 * <file>/path/to/file.txt</file> → [/path/to/file.txt](/path/to/file.txt)
 */
function preprocessFileTags(content: string): string {
  // Match <file>/path/to/file.txt</file> or <file path="/path">name</file>
  return content.replace(/<file(?:\s+path="([^"]*)")?>([^<]*)<\/file>/gi, (_match, path, _content) => {
    const filePath = path || _content.trim();
    return `[${filePath}](${filePath})`;
  });
}

export const MarkdownRenderer: React.FC<MarkdownRendererProps> = ({
  content,
  className,
  onFileClick,
  workspacePath,
}) => {
  // Preprocess content to convert <file> tags to Markdown links
  const processedContent = preprocessFileTags(content);

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

            const text = String(children ?? '');
            const [copied, setCopied] = useState(false);
            const handleCopy = async () => {
              try {
                await navigator.clipboard.writeText(text);
                setCopied(true);
                window.setTimeout(() => setCopied(false), 1400);
              } catch {
                // 静默失败,旧版浏览器或权限缺失
              }
            };

            return (
              <div className={styles.codeBlockWrapper}>
                <div className={styles.codeBlockHeader}>
                  {match && <span className={styles.codeLang}>{match[1]}</span>}
                  <button
                    type="button"
                    className={styles.copyButton}
                    onClick={handleCopy}
                    aria-label="复制代码"
                    title="复制代码"
                  >
                    {copied ? (
                      <Check size={12} className={styles.copyIcon} />
                    ) : (
                      <Copy size={12} className={styles.copyIcon} />
                    )}
                    <span>{copied ? '已复制' : '复制'}</span>
                  </button>
                </div>
                <pre className={styles.codeBlock}>
                  <code className={match ? `language-${match[1]}` : ''} {...props}>
                    {children}
                  </code>
                </pre>
              </div>
            );
          },
          a({ href, children, ...props }) {
            // Decode URL-encoded paths (e.g. %E6%B5%8B%E8%AF%95 -> 测试3)
            const decodedHref = href ? decodeURIComponent(href) : href;
            // Check if this is a file path link (starts with / or ~)
            const isFilePath = decodedHref?.startsWith('/') || decodedHref?.startsWith('~') || /^[A-Za-z]:\\/.test(decodedHref || '');
            const isExternal = href?.startsWith('http://') || href?.startsWith('https://');

            if (isFilePath && onFileClick) {
              const handleClick = (e: React.MouseEvent) => {
                e.preventDefault();
                let fullPath = decodedHref!;
                if (!fullPath.startsWith('/') && !fullPath.startsWith('~') && workspacePath) {
                  fullPath = `${workspacePath}/${fullPath}`;
                }
                onFileClick(fullPath);
              };

              // Extract just the filename for display
              const fileName = decodedHref!.split('/').pop() || decodedHref!;

              return (
                <button
                  type="button"
                  className={styles.filePathTag}
                  onClick={handleClick}
                  title={`点击打开文件: ${decodedHref}`}
                >
                  {fileName}
                </button>
              );
            }

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
        {processedContent}
      </ReactMarkdown>
    </div>
  );
};
