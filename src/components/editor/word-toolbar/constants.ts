// Shared constants for WordToolbar.
//
// Centralising the long lists (font families, colour palettes, paragraph
// styles, zoom levels, etc.) keeps WordToolbar.tsx focused on layout +
// behaviour and avoids re-declaring the same arrays in multiple sibling
// components that previously lived inside the file. Each constant is a
// `const` rather than re-exported so consumers can import only what they
// need without dragging the whole toolbar into their bundle.

export const FONT_FAMILIES = [
  'Microsoft YaHei',
  'SimSun',
  'SimHei',
  'KaiTi',
  'FangSong',
  'Arial',
  'Times New Roman',
  'Calibri',
  'Helvetica',
  'Georgia',
  'Tahoma',
  'Verdana',
];

export const FONT_SIZES_PT = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 48, 72, 96];

export const TEXT_COLORS = [
  '#000000', '#434343', '#666666', '#999999', '#B7B7B7', '#CCCCCC', '#D9D9D9', '#EFEFEF', '#F3F3F3', '#FFFFFF',
  '#980000', '#FF0000', '#FF9900', '#FFFF00', '#00FF00', '#00FFFF', '#4A86E8', '#0000FF', '#9900FF', '#FF00FF',
  '#E6B8B7', '#F4CCCC', '#FCE5CD', '#FFF2CC', '#D9EAD3', '#D0E0E3', '#C9DAF8', '#CFE2F3', '#D9D2E9', '#EAD1DC',
];

export const HIGHLIGHT_COLORS = [
  'none', 'yellow', 'green', 'cyan', 'magenta', 'red', 'blue', 'darkBlue', 'darkCyan', 'darkGreen',
  'darkMagenta', 'darkRed', 'darkYellow', 'darkGray', 'lightGray', 'black', 'white',
];

export const PARAGRAPH_STYLES: Array<{ value: string; label: string }> = [
  { value: 'Normal', label: '正文' },
  { value: 'Heading1', label: '标题 1' },
  { value: 'Heading2', label: '标题 2' },
  { value: 'Heading3', label: '标题 3' },
  { value: 'Heading4', label: '标题 4' },
  { value: 'Heading5', label: '标题 5' },
  { value: 'Heading6', label: '标题 6' },
  { value: 'Title', label: '标题' },
  { value: 'Subtitle', label: '副标题' },
  { value: 'Quote', label: '引用' },
  { value: 'IntenseQuote', label: '明显引用' },
  { value: 'ListParagraph', label: '列表段落' },
  { value: 'NoSpacing', label: '无间距' },
];

export const ZOOM_LEVELS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 2.5, 3];

export const LINE_SPACING_OPTIONS = [
  { value: '1', label: '1.0' },
  { value: '1.15', label: '1.15' },
  { value: '1.5', label: '1.5' },
  { value: '2', label: '2.0' },
  { value: '2.5', label: '2.5' },
  { value: '3', label: '3.0' },
];

export const SYMBOLS = [
  '§', '©', '®', '™', '¶', '†', '‡', '•', '…', '–', '—', '·',
  '€', '£', '¥', '¢', '₹', '₽', '₩', '₪', '¢', '¤',
  '°', '′', '″', 'µ', 'π', 'Ω', '∞', '√', '÷', '×', '±', '≈', '≠', '≤', '≥', '∑',
  '←', '→', '↑', '↓', '↔', '⇒', '⇔',
  '★', '☆', '♠', '♡', '♢', '♣', '♪', '♫', '♥', '♦', '♀', '♂',
  '☺', '☻', '✓', '✗', '✔', '✘',
  '☎', '✉', '✂', '✏', '✒', '⚙', '⚡', '⚠', '☂', '❤',
];

export const WATERMARK_COLORS = [
  '#C0C0C0', '#808080', '#404040', '#000000',
  '#D9D9D9', '#F2F2F2', '#FFFFFF',
  '#FF6B6B', '#4ECDC4', '#45B7D1', '#FFA07A',
  '#980000', '#0066CC', '#3D9970', '#FFB400',
];

export const WATERMARK_FONTS = [
  'Microsoft YaHei',
  'SimSun',
  'SimHei',
  'KaiTi',
  'Arial',
  'Calibri',
  'Times New Roman',
  'Helvetica',
  'Georgia',
];

export const MATH_PRESETS = [
  { label: 'x²+y²=r²', latex: 'x^2 + y^2 = r^2' },
  { label: '√(a²+b²)', latex: '\\sqrt{a^2 + b^2}' },
  { label: 'a/b 分数', latex: '\\frac{a}{b}' },
  { label: 'Σ 求和', latex: '\\sum_{i=1}^{n} x_i' },
  { label: '∫ 积分', latex: '\\int_a^b f(x)\\,dx' },
  { label: 'lim 极限', latex: '\\lim_{x \\to 0} \\frac{\\sin x}{x}' },
  { label: '矩阵', latex: '\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}' },
  { label: '希腊 αβγ', latex: '\\alpha\\,\\beta\\,\\gamma' },
];