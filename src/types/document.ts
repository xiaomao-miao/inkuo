// Document model — block tree used by the document loader and the
// CodeMirror-backed editor.

export interface Document {
  id: string;
  path: string;
  doc_type: DocumentType;
  title: string;
  blocks: Block[];
  updated_at: string;
  hash: string;
}

export type DocumentType = 'Markdown' | 'Word' | 'Excel' | 'PlainText';

export interface Block {
  id: string;
  kind: BlockKind;
  range: Range;
  text: string;
  metadata: Record<string, unknown>;
}

export type BlockKind =
  | 'Paragraph'
  | { Heading: number }
  | { CodeBlock: string | null }
  | { List: boolean }
  | 'ListItem'
  | 'Table'
  | 'TableRow'
  | 'TableCell'
  | 'Blockquote'
  | 'HorizontalRule';

export interface Range {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}