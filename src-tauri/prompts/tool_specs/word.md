# Word document (.docx) tools

Before modifying a Word document, **almost always call `read_office_file` first** — you need the stable `id`s of its `elements` to make precise edits.

## create_word_doc (unified interface)
Create, modify, append, or delete content in a .docx. A single tool covers every operation.

**Parameters**:
- `path` (**required on every call, including append calls**)
- `title` (optional, new files only) — document title (auto-renders as a `Title`-styled paragraph). **Do NOT also add a Heading1/Title paragraph with the same text in `elements[]`; the title paragraph is generated for you.**
- `elements` (optional array) — see below
- `deletes` (optional, array of element ids) — batch delete by id
- `append` (bool, optional) — when true, new elements are appended to the end
- `sections` (optional array) — see "Sections, headers, footers" below
- `headers` (optional array) — see "Sections, headers, footers" below
- `footers` (optional array) — see "Sections, headers, footers" below

> ⚠️ **`path` is stateless.** The backend does not remember the file path between tool calls. You must repeat the full absolute `path` on **every** invocation — including each `append: true` chunk in long-document generation. Omitting `path` returns an error and the call fails.

**Paragraph element (`type: "paragraph"`) fields**:
- `id` (optional) — stable id from `read_office_file`. Provided = modify that paragraph; absent = create a new one.
- `text` (optional) — paragraph text. **Omit on modify = keep original text.** Required when creating (or use `""` for an intentional blank line).
- `style` (optional) — `"Title"` / `"Heading1"` / `"Heading2"` / `"Heading3"` / `"Normal"`. **Omit on modify = keep original style.** Almost every paragraph should specify a style.
- `runs` (optional) — rich-text segments `[{text, bold?, italic?, underline?, strikethrough?, font_size?, color?, font_name?, highlight?, vert_align?, field?}]`. **Omit on modify = keep original inline formatting.** Once provided, runs **completely replace** the original list.
  - `font_size` is in **half-points** (24 = 12pt, 28 = 14pt, 32 = 16pt, 36 = 18pt, 44 = 22pt, 56 = 28pt).
  - `color` is a 6-character hex RGB string, e.g. `"FF0000"` for red.
  - `vert_align` is `"superscript"` or `"subscript"`.
  - `field` is a Word field code; see "Field codes (域代码)" below.
- `alignment` (optional) — `"left"` | `"right"` | `"center"` | `"both"` (justify) | `"distribute"`.
- `text_direction` (optional) — `"horizontal"` (default) | `"vertical"` | `"verticalRightToLeft"` | `"verticalLeftToRight"` | `"rotate90"` | `"rotate270"`. Use `"verticalRightToLeft"` for traditional Chinese / Japanese vertical writing.
- `numbering` (optional) — `{num_id, level}` for list items. `num_id: 1` = bulleted, `num_id: 2` = decimal-numbered.
- `anchor_id` + `position` — `"before"` | `"after"` to insert relative to an existing element.
- `action: "delete"` — delete the element with this id.

**Table element (`type: "table"`) fields**:
- `id` (optional) — modify an existing table.
- `header` (required) — column labels (becomes the first row).
- `rows` (required) — data rows as a 2-D string array, e.g. `[["metric", "95%"], ["score", "88%"]]`.
- `anchor_id` + `position` — relative insertion.
- `action: "delete"`.

**Image element (`type: "image"`) fields**:
- `path` (required) — absolute local path to a png / jpeg / jpg / gif file.
- `width_emu` / `height_emu` (required) — dimensions in EMU (914400 = 1 inch, 360000 = 1 cm).
- `id`, `anchor_id`, `position` — same as paragraphs/tables.

### Visual asset workflow

For visual-friendly deliverables, do not stop at headings and coloured tables. Use the Word expert's asset tools to create or source purposeful visuals, inspect their real pixels, then embed them as image elements:

- Vector/diagram: `create_svg` → `read_image` → `svg_to_png` → `read_image` → `create_word_doc` image element.
- Process/architecture diagram: `render_mermaid` directly to PNG → `read_image` → embed.
- Illustration/cover image: `generate_image` → `read_image` → embed.

The DOCX writer accepts PNG/JPEG/JPG/GIF, not raw SVG. Preserve the source asset next to the document, use absolute paths, keep aspect ratio, and place a caption near each non-decorative visual. Visuals should explain or support content; conservative legal/memo genres may intentionally use none.

**Key behavioral rules**:
1. Always specify `style` for every paragraph (`Heading1/2/3` for section titles, `Normal` for body).
2. **Do not duplicate the document title.** If you pass `title="..."`, the backend already inserts a `Title`-styled paragraph for it. Do not also add a `Heading1`/`Title` paragraph with the same text in `elements[]`, and do not repeat the title as the first `Heading1` of the first section when appending.
3. Omitted fields on modify = preserve original — this is the safe default for "edit just one thing".
4. When you supply `runs`, they replace the entire run list. To bold a single word you must echo the other runs' text.
5. Always `read_office_file` first — otherwise ids won't line up and edits will land in the wrong paragraph.

## Design-system components (recommended for new documents)
> **Preferred path for new documents.** In addition to the low-level `paragraph` / `table` / `image` elements, `create_word_doc` accepts a set of **component blocks** that produce brand-styled output (custom font scales, palette colours, header-repeat, zebra striping, callout containers, code blocks). Use these instead of building the same look by hand with `paragraph` + `style` + `runs`.

**Component element types** — each entry in `elements[]` may set `type` to one of:
- `cover` — `{type: "cover", id?, title, subtitle?}`. Oversized centred cover-page title + subtitle + spacer. Use once at the top of a new document.
- `chapter` — `{type: "chapter", id?, title}`. Chapter title (maps to `ChapterTitle` style).
- `heading` — `{type: "heading", id?, level: 1|2|3, text}`. `1` = chapter (ChapterTitle), `2` = section (SectionTitle), `3` = subsection (SubsectionTitle).
- `body` — `{type: "body", id?, text}` or `{type: "body", id?, runs: [{text, bold?, italic?}, ...]}`. Body paragraph (BodyParagraph style). Use `runs` for inline rich text.
- `bullet_list` — `{type: "bullet_list", id_prefix, items: [string, ...]}`. One bulleted paragraph per item (uses numbering `num_id: 1`).
- `ordered_list` — `{type: "ordered_list", id_prefix, items: [string, ...]}`. One ordered paragraph per item (uses numbering `num_id: 2`).
- `styled_table` — `{type: "styled_table", id?, headers: [string, ...], rows: [[string, ...], ...], style?: {...}}`. Brand-styled table with optional zebra striping + header repeat. `style` fields: `{header_fill?, zebra_fill?, border_color?, header_text_color?, repeat_header?, zebra?}` — all optional, sensible defaults from the active palette.
- `callout` — `{type: "callout", id?, level: "info"|"warning"|"important"|"tip", title, body?, body_lines?: [string, ...]}`. Coloured-background callout with level-matching accent. Use `body` for single-line text, `body_lines` for multi-line.
- `code` — `{type: "code", id?, lines: [string, ...], language?}`. Monospace code block with uniform background and optional language label.
- `page_break` — `{type: "page_break", id?}`. Force a hard page break.

**Insertion semantics**: component blocks are **append-only**. Each block expands into a self-contained batch of paragraphs/tables that the tool appends to the end of the document (or after the existing content when modifying). `anchor_id` / `position` are recorded but currently ignored for component blocks — if you need per-paragraph positioning, fall back to a low-level `paragraph` element.

**Mixing high-level and low-level**: you can mix `type: "chapter"` and `type: "paragraph"` in the same `elements[]` array. The legacy elements are still processed exactly as before.

**Example — multi-chapter report with brand styling**:
```json
{
  "path": "/Users/me/docs/annual-report.docx",
  "elements": [
    {"type": "cover", "id": "cover1", "title": "Annual Report 2026", "subtitle": "InkUO Inc."},
    {"type": "chapter", "id": "ch1", "title": "Overview"},
    {"type": "body", "id": "p1", "text": "This year InkUO shipped 12 major features."},
    {"type": "callout", "id": "cal1", "level": "info", "title": "Growth", "body": "Revenue grew 47% YoY."},
    {"type": "chapter", "id": "ch2", "title": "Engineering"},
    {"type": "heading", "id": "h2", "level": 2, "text": "Highlights"},
    {"type": "bullet_list", "id_prefix": "bl", "items": ["Shipped 12 features", "Cut p95 latency by 35%", "Reduced infra cost by 18%"]},
    {"type": "styled_table", "id": "t1", "headers": ["metric", "value"], "rows": [["users", "12k"], ["revenue", "$1.4M"]]},
    {"type": "code", "id": "code1", "lines": ["fn main() { println!(\"hi\"); }"], "language": "rust"}
  ]
}
```
The resulting `.docx` carries the brand palette throughout — coloured headings, brand-coloured table header with white text, zebra-striped body rows, an info callout with brand accent, and a Rust-labelled code block — all without any manual `style` + `runs` wiring.

## Sections, headers, footers
Modern Word documents are partitioned into **sections**, each with its own page size, orientation, margins, text direction, columns, and header/footer references. The `create_word_doc` tool exposes this directly via three top-level arrays.

### `sections[]` — page-level layout
Each entry is one `<w:sectPr>` block. The LAST entry is the trailing sectPr (controls the final page layout). Earlier entries are embedded as "next-page section breaks" inside the last paragraph of their section.

Common fields (all optional except `id`):
- `id` (string, **required**) — stable id used to reference this section.
- `section_type` — `"nextPage"` (default) | `"continuous"` | `"evenPage"` | `"oddPage"` | `"nextColumn"`. `"continuous"` means no page break — useful for switching column count mid-page.
- `page_size_mm` — `{width, height, orient?}` in millimetres. E.g. `{width: 210, height: 297}` = A4 portrait. `orient: "landscape"` swaps the long axis.
- `page_size_twips` — `{width, height, orient?}` in twips (1 inch = 1440 twips, 1 cm ≈ 567 twips). Use this when you know exact dimensions in twips; otherwise `page_size_mm` is friendlier.
- `margins` — `{top, right, bottom, left, header?, footer?, gutter?}` in twips. Defaults to 1440 (1 inch) on all sides if omitted.
- `text_direction` — `"horizontal"` (default) | `"verticalRightToLeft"` | `"verticalLeftToRight"`. Sets `<w:textDirection>` at section level — applies to all paragraphs that don't override it.
- `title_pg` (bool) — when `true`, the first page of the section uses a different header/footer (cover page). Pair with `header_refs` / `footer_refs` that include `kind: "first"`.
- `cols` — number of text columns. `1` = single column. `2` = two columns. `3+` for newsletters / brochures.
- `page_num_start` — restart page numbering from this number. Omit to continue from previous section. `1` is the typical "cover page counts as page 1" reset.
- `page_num_format` — `"decimal"` (default) | `"upperRoman"` | `"lowerRoman"` | `"upperLetter"` | `"lowerLetter"`. E.g. preface pages often use `lowerRoman` (`i`, `ii`, `iii`…).
- `header_refs` — array of `{header_id, kind?}` where `kind` is `"default"` (default) | `"first"` | `"even"`. `header_id` must match an entry in the top-level `headers[]` array.
- `footer_refs` — same shape, but referencing `footers[]` entries.

### `headers[]` and `footers[]` — header/footer parts
Each entry becomes one `word/headerN.xml` (or `footerN.xml`) file. Shape:
- `id` (string, **required**) — used by `header_refs` / `footer_refs` above.
- `paragraphs` (array) — same paragraph shape as `elements[]`. Common contents:
  - Page header: a single `runs: [{text: "Chapter Title", bold: true}]` paragraph.
  - Page footer: a paragraph like `runs: [{text: "Page "}, {text: "", field: {kind: "page"}}, {text: " of "}, {text: "", field: {kind: "numpages"}}]`.
  - Cover page: empty paragraph (or a small logo) referenced via `kind: "first"`.

> **Auto-injected page-number footer for brand-styled documents.** If you use any design-system component (cover, chapter, styled_table, callout, code) and you don't supply your own `footers[]`, the backend auto-injects a `第 X 页 / 共 Y 页` footer into every section. To override it, pass your own `footers[]` (e.g. `[{id: "main", paragraphs: [{text: "", runs: [{text: "Page "}, {text: "", field: {kind: "page"}}], alignment: "center"}]}]`) and reference it from `sections[].footer_refs`.

> Headers/footers can carry **text only** in v1. Images, tables, and complex layouts inside headers/footers are not yet supported.

### Field codes (域代码)
A run with `field` set renders as a Word field code instead of plain text. This is what makes page numbers, total page counts, dates, and other dynamic content work in Word.

Shape: `{kind: "<one of: page | numpages | date | time | author | title | custom>", format?: "<format>", instr?: "<raw field instruction>"}`

Common `kind` values:
- `"page"` — current page number (`PAGE` field). Resolves to e.g. `3`.
- `"numpages"` — total page count (`NUMPAGES` field). Resolves to e.g. `42`.
- `"date"` — current date (`DATE` field). Optional `format` (e.g. `"yyyy-MM-dd"`, `"yyyy年M月d日"`) controls rendering.
- `"time"` — current time (`TIME` field). Optional `format` (e.g. `"HH:mm"`).
- `"author"` — document author (`AUTHOR` field).
- `"title"` — document title (`TITLE` field).
- `"custom"` — arbitrary field. Provide `instr` with the raw field instruction text, e.g. `instr: "DOCPROPERTY Company"` or `instr: "HYPERLINK \"https://example.com\""`.

The run's `text` is ignored when `field` is set; only the resolved value shows in Word. For example, to render "Page 3 of 42", use four runs in sequence: `{text: "Page "}`, `{text: "", field: {kind: "page"}}`, `{text: " of "}`, `{text: "", field: {kind: "numpages"}}`.

## Long-document incremental generation
For documents expected to have **~2000+ characters**, build incrementally instead of all at once to keep each tool call manageable.

1. `create_word_doc` with `path` and `title="..."` to create the file header (this generates the document's Title paragraph automatically). If the document has headers/footers, pass them in this same call (see Example 2 below).
2. For each section, call `create_word_doc` with `append: true` and the new `elements[]` (~1500-2000 characters of text per chunk). **The first section starts directly with a `Heading1` paragraph for the first section title — do NOT repeat the document `title` here as another `Heading1`/`Title`.**
3. Repeat for subsequent sections.

> ⚠️ **No duplicate top-level titles.** The document `title` you pass in step 1 already produces a `Title`-styled paragraph at the top of the file. When you start the first section in step 2, the first `Heading1` should be the **first section's** heading (e.g. "Overview" or "Introduction"), not a repetition of the document title.

> ⚠️ **The `path` field is required on every chunk above.** The backend does not store path between calls. If you forget `path` on a follow-up call you will get `Missing required field 'path'` and have to retry.

## read_office_file (Word)
For .docx inputs returns:
- `text_content` — full text as a string.
- `elements` — structured element array, each paragraph / table with a stable `id` (used by `create_word_doc`).
- If the file has sections / headers / footers, those are NOT in `elements[]` — they are applied at the document level. Use `inspect_office` to detect their presence; modify them by passing top-level `sections` / `headers` / `footers` in a `create_word_doc` call (they fully replace what's there).

## inspect_office (Word)
For .docx files use `format="docx", mode="info"`. Returns paragraph count, table count, word count, whether headers / footers / images / styles exist — does NOT return body content. **Use before opening a large file to gauge its size.**
- Parameters: `path`, `format="docx"`, `mode="info"`.

## compare_word_docs
Compare two .docx files; return a structured diff (`added[]` / `removed[]` / `modified[]` + summary string).

---

# Examples

## Example 1 — Vertical-text Chinese cover page (竖排封面)
A one-call document with the title set vertically (top-to-bottom, right-to-left — the traditional Chinese / Japanese style), A4 portrait, narrow margins, and no header/footer.

```json
{
  "path": "/Users/me/docs/cover.docx",
  "title": "古诗鉴赏",
  "elements": [
    {
      "style": "Heading1",
      "text": "登鹳雀楼",
      "alignment": "center",
      "text_direction": "verticalRightToLeft"
    },
    {
      "style": "Normal",
      "text": "白日依山尽，黄河入海流。",
      "alignment": "center",
      "text_direction": "verticalRightToLeft"
    },
    {
      "style": "Normal",
      "text": "欲穷千里目，更上一层楼。",
      "alignment": "center",
      "text_direction": "verticalRightToLeft"
    }
  ],
  "sections": [
    {
      "id": "main",
      "page_size_mm": { "width": 210, "height": 297, "orient": "portrait" },
      "margins": { "top": 1800, "right": 1800, "bottom": 1800, "left": 1800 }
    }
  ]
}
```

The four characters of "登鹳雀楼" stack top-to-bottom in the centre, with each couplet of the poem also reading top-to-bottom on its own column. Resize margins to taste.

## Example 2 — Multi-section report with page numbers in the footer
A landscape cover page (section 1, no header/footer) + a portrait body (section 2) with "Page X of Y" in the footer and a chapter title in the header.

```json
{
  "path": "/Users/me/docs/report.docx",
  "title": "Q3 Engineering Report",
  "elements": [
    {
      "style": "Normal",
      "text": "Internal — Confidential",
      "alignment": "center"
    }
  ],
  "headers": [
    {
      "id": "body_header",
      "paragraphs": [
        {
          "style": "Header",
          "alignment": "right",
          "runs": [
            { "text": "Q3 Engineering Report", "italic": true, "color": "595959" }
          ]
        }
      ]
    }
  ],
  "footers": [
    {
      "id": "body_footer",
      "paragraphs": [
        {
          "style": "Footer",
          "alignment": "center",
          "runs": [
            { "text": "Page " },
            { "text": "", "field": { "kind": "page" } },
            { "text": " of " },
            { "text": "", "field": { "kind": "numpages" } }
          ]
        }
      ]
    }
  ],
  "sections": [
    {
      "id": "cover",
      "page_size_mm": { "width": 297, "height": 210, "orient": "landscape" },
      "title_pg": true,
      "page_num_format": "lowerRoman"
    },
    {
      "id": "body",
      "page_size_mm": { "width": 210, "height": 297, "orient": "portrait" },
      "page_num_start": 1,
      "page_num_format": "decimal",
      "header_refs": [{ "header_id": "body_header", "kind": "default" }],
      "footer_refs": [{ "footer_id": "body_footer", "kind": "default" }]
    }
  ]
}
```

The cover page is landscape; the body (everything after the next-page section break) is portrait with "Q3 Engineering Report" in the header and "Page X of Y" in the footer. To continue building the body, call `create_word_doc` with `path`, `append: true`, and new `elements[]` (do NOT re-pass `sections` / `headers` / `footers` on appends unless you want to change them — the existing layout is preserved).

## Example 3 — Two-column newsletter with section break
A single-section document whose second half switches from one column to two columns mid-page (using a `continuous` section break).

```json
{
  "path": "/Users/me/docs/newsletter.docx",
  "title": "Inkwell Weekly — Issue 42",
  "elements": [
    { "style": "Normal", "text": "This is the single-column editorial intro that spans the full page width. Use it for the lead paragraph and any callouts." },
    { "style": "Heading2", "text": "Department Updates" },
    { "style": "Normal", "text": "Engineering shipped 12 features this sprint." },
    { "style": "Normal", "text": "Design shipped 4 new component variants." },
    { "style": "Heading2", "text": "Calendar" },
    { "style": "Normal", "text": "Friday: company all-hands at 4pm." },
    { "style": "Normal", "text": "Next Tuesday: design review for v3." }
  ],
  "sections": [
    {
      "id": "intro",
      "page_size_mm": { "width": 210, "height": 297 },
      "margins": { "top": 1080, "right": 1080, "bottom": 1080, "left": 1080 },
      "cols": 1
    },
    {
      "id": "two_col",
      "section_type": "continuous",
      "cols": 2
    }
  ]
}
```

The first section is single-column for the intro + first heading. The second section (`two_col`, type `continuous`) starts mid-page with two columns for the rest of the document. To insert content into a specific section, use the section break as an anchor via `anchor_id` + `position: "after"`.
