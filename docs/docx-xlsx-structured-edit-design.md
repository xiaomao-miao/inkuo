# inkuo docx/xlsx 结构化编辑 — 完整设计方案

版本：1.0
日期：2026-06-03
状态：初稿，待评审

---

## 一、现状分析

### 1.1 当前实现的问题

#### Docx（`src-tauri/src/office/docx.rs`）

| 问题 | 现状 | 影响 |
|------|------|------|
| 整篇文本替换 | `word_document_to_text()` 把所有段落转成一个字符串发给 AI | AI 无法知道段落边界，修改必然是全文替换 |
| 无样式感知 | 只提取了 `style: Option<String>`（样式名），粗体/斜体/字号等全部丢失 | 回写时没有任何样式信息可用 |
| 回写是重新生成 | `write_word_document()` 用固定模板生成全新 OOXML | 原文档所有样式、格式、页眉页脚全部丢失 |
| 表格是启发式 | `word_document_to_text()` 里用启发式算法猜测表格 | 误判率高，复杂表格一定出错 |
| Run 级信息丢失 | 解析时把 `<w:r>` 里的所有文本合并了 | 无法做 run 级别的精确修改 |

#### Xlsx（`src-tauri/src/office/xlsx.rs`）

| 问题 | 现状 | 影响 |
|------|------|------|
| 只读内容 | `read_excel_workbook()` 只提取文本，公式、格式、合并单元格全部丢弃 | AI 无法理解表格结构 |
| 行数上限 | 硬编码 100 行限制 | 大表格数据丢失 |
| 回写是重新创建 | `write_excel_workbook()` 创建全新 xlsx | 原文件所有格式、公式全部丢失 |
| Sheet 概念弱 | 前端 `ExcelEditor` 只是二维字符串数组 | AI 不知道 sheet 结构，无法做跨 sheet 操作 |
| 只写第一个 sheet | `write_excel_workbook` 总是写 "Sheet1" | 多 sheet 文档回写后只剩一个 sheet |

#### AI 与 Diff（`src-tauri/src/agent/tools/office_tools.rs`）

| 问题 | 现状 | 影响 |
|------|------|------|
| 传给 AI 的只有文本 | `read_office_file` 只返回纯文本，结构信息丢失 | AI 不知道在改哪个段落、哪个单元格 |
| Diff 是全文级 | `compute_diff` 是 `similar` 的 line-based diff | 改一句话，全文 diff，用户无法逐块审核 |
| 没有局部修改指令 | AI 只能整文件替换，没有局部 patch 协议 | 无法做到"只改第三段" |

---

## 二、核心设计思路

### 2.1 三层架构

```
展示层（React）
  └── OfficeViewer / DiffOverlay
      ↓（结构化 JSON）
编辑层（Rust - Document Engine）
  └── DocxEditor / XlsxEditor
      ↓（样式感知对象）
存储层（Rust - OOXML / Office XML）
  └── 解析 / 回写（保留结构）
```

关键转变：从"文本 <-> 文本"变成"结构树 <-> 结构树"。

### 2.2 Docx 编辑的核心数据结构

```rust
// 文档层
struct DocxDocument {
    body: Vec<DocxBlock>,       // 所有块级元素
    styles: StyleMap,           // 文档样式定义
}

// 块级元素
enum DocxBlock {
    Paragraph(DocxParagraph),
    Table(DocxTable),
    SectionBreak,               // 分节符（暂时忽略）
}

// 段落
struct DocxParagraph {
    id: String,                 // 唯一标识，用于 diff 定位
    properties: ParagraphProps, // 样式引用、对齐、缩进、行距
    runs: Vec<DocxRun>,         // 文字片段
}

struct DocxRun {
    id: String,
    text: String,
    properties: RunProps,       // 字体、字号、粗体、斜体、颜色
}

// 表格
struct DocxTable {
    id: String,
    properties: TableProps,
    rows: Vec<TableRow>,
}

// 样式信息（只存储有用字段）
struct StyleMap {
    styles: HashMap<String, StyleDef>,
    default_paragraph: ParagraphProps,
    default_run: RunProps,
}

// 样式继承后的最终属性
struct ResolvedParagraphProps {
    style_name: Option<String>,
    alignment: Option<Alignment>,
    indent: Option<Indent>,
    spacing: Option<Spacing>,
    // 继承自 style + 覆盖
}

struct ResolvedRunProps {
    font: Option<String>,
    size: Option<i32>,          // 半磅单位（Word 内部单位）
    bold: bool,
    italic: bool,
    color: Option<String>,
}
```

### 2.3 Xlsx 编辑的核心数据结构

```rust
struct XlsxWorkbook {
    sheets: Vec<XlsxSheet>,
}

struct XlsxSheet {
    name: String,
    state: SheetState,          // visible / hidden / veryHidden
    merged_cells: Vec<Range>,
    defined_names: Vec<DefinedName>, // 命名区域（用于公式）
    tables: Vec<XlsxTable>,     // 结构化表格对象
    data: Grid<Cell>,           // 单元格网格
}

struct Cell {
    address: CellAddress,       // A1, B2 等
    value: CellValue,
    formula: Option<String>,
    style: Option<CellStyle>,   // 数字格式、填充、边框、字体、对齐
}

struct CellStyle {
    number_format: String,      // "0.00%", "yyyy-mm-dd" 等
    fill: Option<Fill>,
    border: Option<Border>,
    font: Option<Font>,
    alignment: Option<Alignment>,
}
```

---

## 三、Docx 分步实现方案

### 阶段 1：段落级解析与最小回写（优先做）

**目标：能够打开一个 docx，识别段落，局部修改，回写后样式尽量保留。**

#### 3.1.1 增强解析（`docx.rs`）

修改 `read_word_document()` 输出结构化数据：

```
输入：docx 字节流
输出：DocxDocument（带完整段落树）
```

解析步骤：
1. 用 ZIP 读取 `word/document.xml`
2. 用 `quick-xml` 遍历所有 `<w:p>`（段落）和 `<w:tbl>`（表格）
3. 每个段落：
   - 提取 `pPr` → 段落属性（样式名、对齐、缩进、行距）
   - 遍历 `<w:r>` → 提取每个 run 的文本和 `rPr`（字体、字号、粗体、斜体、颜色）
4. 每个表格：
   - 提取 `<w:tbl>` → `<w:tr>` → `<w:tc>` → 遍历 cell 内容
5. 同时读取 `word/styles.xml`，解析样式定义，建立 `StyleMap`

关键改变：**不再是所有文本合并成一个 String，而是保持树状结构。**

#### 3.1.2 前端展示层

在 `OfficeViewer.tsx` 里新增一个"结构视图"：

```tsx
interface StructuredDocxViewerProps {
  document: DocxDocument;
  selectedBlockIds: Set<string>;
  onBlockSelect: (blockId: string, multi: boolean) => void;
  onInlineEdit: (blockId: string, newRuns: DocxRun[]) => void;
}
```

展示方式：
- 每段左边显示**段落 ID**（隐藏但可 hover）和**样式标签**（如 "Heading 1"、"Quote"）
- Run 级别粗体/斜体保留（前端渲染）
- 选中一段时，该段高亮，显示"AI 编辑"按钮
- 支持多段选择

#### 3.1.3 AI 上下文构造

传给 AI 的不再是纯文本，而是**结构化的段落描述**：

```json
{
  "document": {
    "title": "xxx.docx",
    "blocks": [
      {
        "id": "p-001",
        "type": "paragraph",
        "style": "Normal",
        "runs": [
          { "text": "这是第一段", "bold": false, "italic": false }
        ]
      },
      {
        "id": "p-002",
        "type": "paragraph",
        "style": "Heading1",
        "runs": [
          { "text": "第一章", "bold": true, "italic": false }
        ]
      },
      {
        "id": "t-001",
        "type": "table",
        "rows": 3,
        "cols": 3,
        "sample": [["A1", "B1", "C1"], ["A2", "B2", "C2"], ["..."]]
      }
    ]
  }
}
```

AI 收到的指令格式改为：

```json
{
  "instruction": "把第二段改成更正式的语气",
  "modify": [
    { "block_id": "p-002", "action": "replace", "new_runs": [...] }
  ]
}
```

#### 3.1.4 最小回写（保守策略）

**目标：只替换文本内容，段落属性和 run 属性尽量继承原文。**

回写策略：
1. 读取原 docx 的 XML
2. 找到对应 `<w:p>`（按顺序匹配 block id）
3. 只修改 `<w:t>` 里的文本内容
4. 如果 run 数变了（比如改后字数差太多），合并/拆分 run 时**优先保留属性**
5. 写回原文件，自动创建 `.docx.bak`

这个阶段**不做**：样式重映射、复杂嵌套、页眉页脚。

---

### 阶段 2：样式保持与增强（第二阶段）

#### 3.2.1 样式继承引擎

定义一个"属性合并"函数：

```
最终属性 = 文档默认属性 + 段落样式属性 + 直接覆盖
```

前端需要展示：
- 当前段落用了什么样式
- 样式继承链（Normal → 自定义样式）
- 如果 AI 修改后样式可能变化，给出警告

#### 3.2.2 AI 输出协议升级

定义一个标准修改指令格式：

```json
{
  "action": "modify_block",
  "block_id": "p-003",
  "style_mode": "inherit",      // inherit | specify | auto
  "new_content": {
    "runs": [
      { "text": "新文本第一部分", "inherit": true },
      { "text": "加粗部分", "bold": true, "inherit": true },
      { "text": "继续普通文本", "inherit": true }
    ]
  },
  "preserve_structure": true
}
```

#### 3.2.3 粗体/斜体保留策略

当前 `docx.rs` 里没有记录 run 级别的粗体/斜体。需要增强解析：

- 遍历 `<w:rPr>`，提取：
  - `<w:b/>` → bold = true
  - `<w:i/>` → italic = true
  - `<w:color w:val="FF0000"/>` → color
  - `<w:sz w:val="24"/>` → size (half-points)
  - `<w:rFonts w:ascii="Arial"/>` → font

---

### 阶段 3：完整富文本视图（第三阶段）

- 实现富文本 WYSIWYG 编辑器（用 `@eigenpal/docx-editor-react` 的更多能力）
- 支持直接在富文本视图里选区、触发 Cmd+K
- 支持两个视图切换：Markdown 视图 / 富文本视图
- 支持更多样式：缩进、项目符号编号、页眉样式

---

## 四、Xlsx 分步实现方案

### 阶段 1：结构化解析与单元格级修改（优先做）

#### 4.1.1 增强解析（`xlsx.rs`）

修改 `read_excel_workbook()` 输出结构化数据：

```rust
struct XlsxWorkbook {
    sheets: Vec<XlsxSheet>,
    defined_names: Vec<DefinedName>,
    shared_strings: Vec<String>,  // xlsx 内部字符串池
}

struct XlsxSheet {
    name: String,
    data: Grid<Cell>,
    merged_cells: Vec<Range>,
    table_parts: Vec<String>,      // 表格对象引用
    auto_filter: Option<Range>,    // 筛选区域
    freeze_panes: Option<CellAddress>, // 冻结窗格
}

struct Cell {
    address: CellAddress,
    value: CellValue,
    formula: Option<String>,
    style: CellStyle,
    cell_type: CellType,          // number / string / boolean / date / error
}

struct CellStyle {
    number_format_index: u32,
    number_format_string: String,  // "0.00", "yyyy-mm-dd", "@"
    fill_pattern: Option<String>,
    fill_fg_color: Option<String>,
    font_index: u16,
    alignment: Option<CellAlignment>,
}
```

解析步骤（使用 `calamine` 已有数据 + 额外读取 `xl/styles.xml` 和 `xl/sharedStrings.xml`）：
1. 用 `calamine` 读取单元格值（已有）
2. 读取 `xl/styles.xml` 解析 `cellXfs` → 获取每个单元格的样式索引
3. 读取 `xl/sharedStrings.xml` 解析字符串池
4. 读取 `xl/worksheets/sheet*.xml` 解析 `mergeCells` → 合并单元格

#### 4.1.2 前端展示

```tsx
interface StructuredExcelViewerProps {
  workbook: XlsxWorkbook;
  activeSheet: number;
  selectedRange: Range | null;
  onRangeSelect: (range: Range) => void;
  onCellEdit: (address: CellAddress, newValue: CellValue) => void;
}
```

展示方式：
- 数据网格 + 表头（冻结首行）
- 合并单元格正确合并显示
- 选中单元格时显示完整信息（值、公式、样式）
- 支持多选区域，然后"AI 编辑选中区域"

#### 4.1.3 AI 上下文

传给 AI 的格式：

```json
{
  "workbook": {
    "sheets": [
      {
        "name": "销售报表",
        "dimensions": { "max_row": 50, "max_col": 8 },
        "data": [
          ["产品", "1月", "2月", "3月"],
          ["A类", "1000", "1200", "1100"],
          ["B类", "800", "900", "950"]
        ],
        "formats": [
          ["General", "货币", "货币", "货币"],
          ["General", "货币", "货币", "货币"]
        ]
      }
    ]
  }
}
```

AI 指令格式：

```json
{
  "instruction": "把第二列的格式改成百分比",
  "modify": {
    "sheet": 0,
    "cells": [
      {
        "address": "B2",
        "action": "set_number_format",
        "value": "0%"
      }
    ]
  }
}
```

#### 4.1.4 保守回写

**目标：只写回修改过的单元格，不动其他任何东西。**

使用 `openpyxl`（Rust）或者自己写 ZIP 修改：
1. 读取原 xlsx（ZIP）
2. 找到 `xl/worksheets/sheet*.xml`
3. 修改对应 `<c>` 元素的属性和内容
4. 如果改了样式，更新 `xl/styles.xml` 相关 xf
5. 写回原文件，创建 `.xlsx.bak`

关键：保留原文件 95%+ 的内容不变。

---

### 阶段 2：公式支持与智能操作（第二阶段）

#### 4.2.1 自然语言操作集

定义一组 AI 可调用的工具函数：

```rust
// Excel 操作函数
fn format_cells(sheet: &mut XlsxSheet, range: Range, format: String);
fn sort_range(sheet: &mut XlsxSheet, range: Range, key_col: usize, ascending: bool);
fn filter_range(sheet: &mut XlsxSheet, range: Range, column: usize, criteria: String);
fn insert_column(sheet: &mut XlsxSheet, after_col: usize);
fn delete_column(sheet: &mut XlsxSheet, col: usize);
fn add_chart(sheet: &mut XlsxSheet, range: Range, chart_type: ChartType);
fn set_header(sheet: &mut XlsxSheet, text: String);
fn apply_conditional_format(sheet: &mut XlsxSheet, range: Range, rule: FormatRule);
```

这些函数在 `office_tools.rs` 里实现，AI 工具调用这些函数，而不是直接生成 JSON。

#### 4.2.2 Diff 增强

Excel 的 diff 不再是文本 diff，而是**单元格 diff**：

```json
{
  "sheet": "销售报表",
  "changes": [
    {
      "address": "C3",
      "old_value": "1000",
      "new_value": "1500",
      "old_style": "number_format: 货币",
      "new_style": "number_format: 货币",
      "type": "value_change"
    },
    {
      "address": "D1",
      "old_value": "3月",
      "new_value": "Q1汇总",
      "type": "value_change"
    },
    {
      "address": "E1:F10",
      "type": "inserted_columns",
      "description": "新增2列：Q1合计、Q1均值"
    }
  ]
}
```

---

## 五、AI 修改协议设计

### 5.1 核心原则

AI 不应该拿到整篇文档然后输出一篇新文档，而应该：
1. 拿到结构化文档
2. 输出**修改指令集**（而不是完整结果）
3. 由本地引擎执行修改
4. 保留原始样式

### 5.2 Docx 修改指令格式

```json
{
  "version": "1.0",
  "document_id": "xxx.docx",
  "modify": [
    {
      "type": "replace_block",
      "block_id": "p-003",
      "style_mode": "inherit",     // inherit | style:name | auto
      "new_runs": [
        {
          "text": "新的段落文本",
          "bold": false,
          "italic": false,
          "inherit_props": true
        },
        {
          "text": "这段加粗",
          "bold": true,
          "inherit_props": true
        }
      ]
    },
    {
      "type": "insert_block",
      "after_block_id": "p-002",
      "style_mode": "inherit_from",
      "new_runs": [...]
    },
    {
      "type": "delete_block",
      "block_id": "p-005"
    },
    {
      "type": "modify_run",
      "block_id": "p-003",
      "run_index": 1,
      "new_text": "新文本"
    }
  ],
  "summary": "修改了1个段落，新增1个段落"
}
```

### 5.3 Xlsx 修改指令格式

```json
{
  "version": "1.0",
  "workbook": "xxx.xlsx",
  "operations": [
    {
      "type": "set_cell_value",
      "sheet": 0,
      "address": "C3",
      "value": 1500,
      "value_type": "number"
    },
    {
      "type": "set_cell_format",
      "sheet": 0,
      "address": "C3",
      "number_format": "¥#,##0"
    },
    {
      "type": "insert_row",
      "sheet": 0,
      "after_row": 5
    },
    {
      "type": "set_cell_formula",
      "sheet": 0,
      "address": "D10",
      "formula": "=SUM(D2:D9)"
    }
  ],
  "summary": "修改了1个单元格数值，设置了格式"
}
```

### 5.4 AI Prompt 改造

需要改造 `src-tauri/src/agent/prompts.rs` 里的 `get_edit_system_prompt()`：

```
你是一个精确的文档编辑器。你不会输出完整文档，只会输出修改指令。

文档结构：
- 文档被分成多个 block（段落或表格）
- 每个 block 有唯一 ID（p-001, p-002, t-001...）
- 文本包含粗体/斜体等 inline 样式信息
- 你可以：替换某个 block 的内容、在 block 之间插入新 block、删除 block

指令格式（必须严格遵循 JSON）：
{
  "modify": [...],
  "summary": "一句话说明做了什么"
}

约束：
1. 尽量使用 style_mode: "inherit"，保留原文样式
2. 不要改动没有让你改动的 block
3. 每个修改的 block 都要给出 block_id
4. summary 必须简洁，10 字以内
```

---

## 六、Diff 展示升级

### 6.1 Docx Diff

不再显示全文 diff，而是**块级 diff**：

```
┌──────────────────────────────────────────┐
│ [p-003] Normal                    +1 -2  │
│ "这是旧的段落文本，包含了一些内容"           │
│ ↓                                      │
│ "这是新的段落文本，已按要求修改了语气"        │
│                                          │
│ [操作] ✓ 接受区块  ✗ 拒绝区块  📋 复制     │
└──────────────────────────────────────────┘

┌──────────────────────────────────────────┐
│ [p-004] Quote                   +3 -1    │
│ 旧内容...                                  │
│ ↓                                      │
│ 新内容（更多段落）                           │
│                                          │
│ [操作] ✓ 接受区块  ✗ 拒绝区块  📋 复制     │
└──────────────────────────────────────────┘

[ 全部接受 ]  [ 全部拒绝 ]
```

### 6.2 Xlsx Diff

单元格级 diff：

```
┌──────────────────────────────────────────────────┐
│ 销售报表 - Sheet1                        +2 -0  │
│                                                  │
│  C3    1000  →  1500          [值变更]           │
│  C3    货币格式  →  货币格式      [格式不变]      │
│                                                  │
│  E1:F1 [新增列]  Q1合计 | Q1均值                  │
│                                                  │
│ [操作] ✓ 接受  ✗ 拒绝  📋 复制                  │
└──────────────────────────────────────────────────┘
```

---

## 七、技术实现路线图

### 优先级排序

```
P0（必须先做，这是 MVP）
├── 1. Docx 结构化解析（段落树 + run 信息）
├── 2. 前端段落选择 UI
├── 3. AI 上下文改为结构化 JSON
├── 4. AI 修改指令协议（JSON patch）
├── 5. Docx 保守回写（只改文本，继承样式）
└── 6. 块级 Diff 展示

P1（第二阶段，核心差异化）
├── 7. Docx 样式继承引擎
├── 8. Xlsx 结构化解析（保留样式和公式）
├── 9. Xlsx 单元格级修改
├── 10. Xlsx 保守回写（只改单元格）
├── 11. Excel Diff 展示
└── 12. AI Excel 自然语言工具集

P2（第三阶段，完整产品）
├── 13. Docx 富文本视图
├── 14. Docx 表格编辑增强
├── 15. Xlsx 多 sheet 管理
├── 16. 样式模板系统
└── 17. 备份与历史管理
```

### 技术依赖

| 阶段 | 依赖项 | 建议 |
|------|--------|------|
| P0 | `quick-xml`（已有） | 增强解析逻辑 |
| P0 | `similar`（已有） | 用于文本级 diff，保留 |
| P0 | 前端 DiffOverlay | 改为块级展示 |
| P1 | `openpyxl` Rust 或手写 ZIP | Excel 回写 |
| P1 | 样式表解析 | `styles.xml` 解析 |
| P1 | 公式解析 | 简单公式支持 |

### Rust crate 推荐

```
// 新增依赖
[dependencies]
serde = "1"              // 已有
serde_json = "1"         // 已有
quick-xml = "0.31"        // 已有，继续用
calamine = "0.21"        // 已有，继续用
openpyxl = "0.6"         // 新增：Excel 写回（需要确认 Rust openpyxl 生态）

// 可选
css-inline = "0.11"      // 可选：样式内联辅助
```

---

## 八、文件修改清单

### `src-tauri/src/office/docx.rs`

| 修改项 | 内容 |
|--------|------|
| `WordParagraph` | 增加 `id: String`、`runs: Vec<DocxRun>` |
| `DocxRun` 新增 | `id`、`bold`、`italic`、`color`、`font`、`size` |
| `WordTable` | 改为结构化 `Vec<TableRow>` |
| `read_word_document()` | 返回 `DocxDocument`（结构化）而非合并文本 |
| `word_document_to_markdown()` | 改为输出结构化 JSON |
| `write_word_document()` | 改为 `apply_patch(doc, modify)`，智能回写 |

### `src-tauri/src/office/xlsx.rs`

| 修改项 | 内容 |
|--------|------|
| `ExcelSheet` | 增加 `data: Grid<Cell>`、`merged_cells`、`styles` |
| `Cell` | 增加 `formula`、`style`、`cell_type` |
| `read_excel_workbook()` | 读取 styles.xml 和 sharedStrings.xml |
| `write_excel_workbook()` | 改为增量写回，只改修改过的单元格 |

### `src-tauri/src/agent/tools/office_tools.rs`

| 修改项 | 内容 |
|--------|------|
| `ReadOfficeFileTool` | 输出结构化 JSON（带 block id、cell address） |
| `WriteOfficeFileTool` | 改为接受修改指令 patch，而非整文件内容 |
| 新增 `ModifyDocxTool` | 专门处理 docx 块级修改 |
| 新增 `ModifyExcelTool` | 专门处理 excel 单元格修改 |

### `src-tauri/src/agent/prompts.rs`

| 修改项 | 内容 |
|--------|------|
| `get_edit_system_prompt()` | 改为输出结构化修改指令 |
| 新增 docx-specific prompt | 段落、run、样式概念 |
| 新增 xlsx-specific prompt | 单元格、sheet、公式概念 |

### `src/components/editor/OfficeViewer.tsx`

| 修改项 | 内容 |
|--------|------|
| `WordDocument` | 改为结构化 `DocxDocument` |
| `Paragraph` | 改为 `DocxParagraph`（带 run 列表） |
| `ExcelWorkbook` | 改为 `XlsxWorkbook` |
| 新增 `BlockSelector` | 段落级选择 UI |
| 新增 `CellRangeSelector` | Excel 单元格选择 UI |

### `src/components/editor/DiffOverlay.tsx`

| 修改项 | 内容 |
|--------|------|
| 改为 `BlockDiffView` | 显示块级 diff 而非行 diff |
| 新增 `ExcelDiffView` | 单元格级 diff 展示 |

---

## 九、关键风险与缓解

| 风险 | 严重程度 | 缓解方案 |
|------|----------|----------|
| Docx 样式解析不完整 | 高 | 先做最小集（粗体/斜体/字号/对齐），其余降级处理 |
| 表格回写错位 | 高 | 保守策略：表格只改文本内容，不动结构 |
| Xlsx 公式被破坏 | 高 | 回写时保留公式字符串，不重新计算 |
| AI 输出指令格式不对 | 中 | 在 Rust 层做 JSON schema 验证 + fallback |
| 合并单元格冲突 | 中 | 限制修改范围，合并单元格区域禁止结构性修改 |
| 大文档性能 | 中 | 分块加载 + 虚拟化渲染 + 渐进式解析 |

---

## 十、效果预览

用户使用流程（目标态）：

```
1. 打开 xxx.docx
   → 显示结构化段落列表，每个段落带样式标签

2. 选中第 3 段
   → 高亮，显示 Cmd+K 按钮

3. 按 Cmd+K，输入"改成更正式的语气，保留所有数据和术语"
   → AI 返回修改指令（只改该段落文本，继承样式）

4. 界面显示块级 diff
   → 红色删 / 绿色增，段落级别
   → 顶部摘要："语气改为正式，新增2句"

5. 按 Tab 接受
   → 文档更新，样式完全保留

6. Ctrl+S 保存
   → 自动创建 .docx.bak
   → 保守回写，格式尽量保留
```
