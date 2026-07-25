import { EditorView, keymap, lineNumbers, drawSelection, rectangularSelection } from '@codemirror/view';
import { Prec, type Extension } from '@codemirror/state';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { history, historyKeymap } from '@codemirror/commands';
import { highlightSelectionMatches, searchKeymap } from '@codemirror/search';
import { inlineDiffTheme } from './inlineDiffDecorations';
import { inlineCompletionDecoration } from '../inline-complete';
import { useInlineCompleteStore } from '../../store';
import type { FileKind } from '../../types';
import { json } from '@codemirror/lang-json';
import { yaml } from '@codemirror/lang-yaml';
import { xml } from '@codemirror/lang-xml';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import { cpp } from '@codemirror/lang-cpp';
import { java } from '@codemirror/lang-java';
import { go } from '@codemirror/lang-go';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { sql } from '@codemirror/lang-sql';
import { php } from '@codemirror/lang-php';

// `@codemirror/language-data`'s `languages` array ships per-language
// metadata + a `load()` async importer. Passing it as `codeLanguages`
// only registers the metadata with CodeMirror's markdown parser; the
// underlying Lezer grammar + StreamLanguage parsers are dynamically
// imported on first use (when a fenced code block is actually rendered).
// This keeps the markdown editor main-chunk small even with 70+
// language definitions reachable from a single import.

/**
 * Map a `FileKind` (or a raw extension) to the most appropriate CodeMirror
 * language extension. Falls back to a plain text view (no language
 * extension) for kinds the editor cannot meaningfully highlight.
 *
 * Lazy-loaded language packages (e.g. `vue`, `svelte`, `markdown`) are
 * pulled via `@codemirror/language-data`'s dynamic `load()` function so
 * the bundle stays small.
 */
export async function languageExtensionForKind(
  kind: FileKind,
  ext: string,
): Promise<Extension | null> {
  switch (kind) {
    case 'markdown': {
      // Markdown files still get the existing markdown extension, which
      // also enables fenced-code highlighting via `languages`.
      return markdown({ base: markdownLanguage, codeLanguages: languages });
    }
    case 'config': {
      if (ext === 'json' || ext === 'jsonc' || ext === 'json5') return json();
      if (ext === 'yaml' || ext === 'yml') return yaml();
      if (ext === 'xml') return xml();
      if (ext === 'toml' || ext === 'ini' || ext === 'env') return null;
      return null;
    }
    case 'code': {
      // Common first-party lang packages cover the most popular formats.
      // For everything else, fall back to the markdown fenced-code
      // language registry, which dynamically imports the matching
      // StreamLanguage parser on first use.
      switch (ext) {
        case 'ts':
        case 'tsx':
        case 'js':
        case 'jsx':
        case 'mjs':
        case 'cjs':
          return javascript({ typescript: ext === 'ts' || ext === 'tsx' });
        case 'py':
          return python();
        case 'rs':
          return rust();
        case 'c':
        case 'h':
        case 'cpp':
        case 'cc':
        case 'cxx':
        case 'hpp':
        case 'hxx':
          return cpp();
        case 'java':
          return java();
        case 'go':
          return go();
        case 'html':
        case 'htm':
        case 'vue':
        case 'svelte':
        case 'astro':
        case 'mdx':
          return html();
        case 'css':
        case 'scss':
        case 'sass':
        case 'less':
          return css();
        case 'sql':
          return sql();
        case 'php':
          return php();
        default: {
          // Lazy-load via `@codemirror/language-data` so we don't have to
          // ship every language parser up-front.
          const meta = languages.find((l) => l.name.toLowerCase() === ext);
          if (meta) {
            try {
              const support = await meta.load();
              if (support) return support;
            } catch {
              // fall through to plain
            }
          }
          return null;
        }
      }
    }
    case 'data': {
      // CSV/TSV are tabular; no first-party Lezer grammar. We use plain
      // text view so the values can still be edited freely.
      return null;
    }
    default:
      return null;
  }
}

export function createEditorTheme() {
  return EditorView.theme({
    '&': {
      height: '100%',
      // Read from the `--editor-font-size` CSS variable so the font
      // size can be tuned at runtime (the View menu's 放大/缩小/重置
      // buttons, the settings panel slider, etc.) without rebuilding
      // the extension. The variable is set on `:root` by `Editor.tsx`
      // from the `editor_font_size` setting; falls back to 14px.
      fontSize: 'var(--editor-font-size, 14px)',
      backgroundColor: 'var(--bg-primary)',
    },
    '&.cm-editor': {
      backgroundColor: 'var(--bg-primary)',
    },
    '.cm-scroller': {
      fontFamily: 'var(--font-mono)',
      backgroundColor: 'var(--bg-primary)',
    },
    '.cm-content': {
      padding: '16px 0',
      backgroundColor: 'var(--bg-primary)',
    },
    '.cm-line': {
      padding: '0 16px',
    },
    '.cm-gutters': {
      backgroundColor: 'var(--bg-secondary)',
      borderRight: '1px solid var(--border-color)',
      color: 'var(--fg-muted)',
    },
    '.cm-lineNumbers .cm-gutterElement': {
      color: 'var(--fg-muted)',
      padding: '0 16px 0 8px',
    },
    '.cm-activeLineGutter': {
      backgroundColor: 'var(--bg-tertiary)',
      color: 'var(--fg-secondary)',
    },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
      backgroundColor: 'var(--selection-bg)',
    },
    '&.cm-focused .cm-cursor': {
      borderLeftColor: 'var(--accent-primary)',
      borderLeftWidth: '2px',
    },
    '&.cm-focused .cm-content ::selection': {
      backgroundColor: 'var(--selection-bg)',
    },
  });
}

export function createInlineCompletionKeymap(autoTriggerStateRef: React.RefObject<{
  timer: ReturnType<typeof setTimeout> | null;
  lastAcceptAt: number;
  destroyed: boolean;
}>) {
  return Prec.highest(
    keymap.of([
      {
        key: 'Tab',
        run: (view) => {
          const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();
          if (!currentCompletion) return false;
          const cursorPosition = view.state.selection.main.head;
          const text = currentCompletion.text;

          if (autoTriggerStateRef.current) {
            autoTriggerStateRef.current.lastAcceptAt = Date.now();
          }

          clearCompletion();
          view.dispatch({
            changes: { from: cursorPosition, insert: text },
            selection: { anchor: cursorPosition + text.length },
            userEvent: 'input.complete',
          });
          return true;
        },
        preventDefault: true,
      },
      {
        key: 'Escape',
        run: () => {
          const { currentCompletion, clearCompletion } = useInlineCompleteStore.getState();
          if (!currentCompletion) return false;
          clearCompletion();
          return true;
        },
      },
    ])
  );
}

export function createEditorExtensions(params: {
  diffDecorationsField: Extension;
  inlineCompletionKeyHandler: Extension;
  inlineAutoTrigger: Extension;
  autoTriggerStateRef: React.RefObject<{
    timer: ReturnType<typeof setTimeout> | null;
    lastAcceptAt: number;
    destroyed: boolean;
  }>;
  /** Optional CodeMirror language extension to apply for the current
   *  file.  When omitted, the editor uses the markdown language
   *  (preserving the legacy behavior).  When provided (e.g. for `.ts`,
   *  `.json`, etc.), it replaces the markdown language pack. */
  language?: Extension | null;
}) {
  const langExt = params.language ?? markdown({ base: markdownLanguage, codeLanguages: languages });
  return [
    langExt,
    lineNumbers(),
    drawSelection(),
    rectangularSelection(),
    // `history()` is the state field that records undo/redo entries;
    // basicSetup has `history: false` so editors that relied on it
    // never had any undo stack. We register it explicitly so Ctrl+Z/Y
    // (via `historyKeymap`, added below) actually do something.
    history(),
    highlightSelectionMatches(),
    inlineDiffTheme,
    params.diffDecorationsField,
    params.inlineCompletionKeyHandler,
    params.inlineAutoTrigger,
    createInlineCompletionKeymap(params.autoTriggerStateRef),
    inlineCompletionDecoration(),
    // `historyKeymap` binds Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z to the
    // undo/redo commands. basicSetup has `searchKeymap: false` so we
    // also add `searchKeymap` from `@codemirror/search` ourselves —
    // this is what binds Ctrl+F and Ctrl+H to openSearchPanel /
    // openReplacePanel, which the menu bars in TitleBar piggy-back on.
    keymap.of([...historyKeymap, ...searchKeymap]),
    createEditorTheme(),
  ];
}
