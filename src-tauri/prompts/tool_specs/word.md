# Word document (.docx) tools

Before modifying a Word document, **almost always call `read_office_file` first** — you need the stable `id`s of its `elements` to make precise edits.

## create_word_doc (unified interface)
Create, modify, append, or delete content in a .docx. A single tool covers every operation.

**Parameters**:
- `path` (required)
- `title` (optional, new files only) — document title (auto-renders as a `Title`-styled paragraph)
- `elements` (optional array) — see below
- `deletes` (optional, array of element ids) — batch delete by id
- `append` (bool, optional) — when true, new elements are appended to the end

**Paragraph element (`type: "paragraph"`) fields**:
- `id` (optional) — stable id from `read_office_file`. Provided = modify that paragraph; absent = create a new one.
- `text` (optional) — paragraph text. **Omit on modify = keep original text.** Required when creating (or use `""` for an intentional blank line).
- `style` (optional) — `"Title"` / `"Heading1"` / `"Heading2"` / `"Heading3"` / `"Normal"`. **Omit on modify = keep original style.** Almost every paragraph should specify a style.
- `runs` (optional) — rich-text segments `[{text, bold?, italic?, underline?, strikethrough?, font_size?, color?, font_name?, highlight?}]`. **Omit on modify = keep original inline formatting.** Once provided, runs **completely replace** the original list.
- `numbering` (optional) — `{num_id, level}` for list items. `num_id: 1` = bulleted, `num_id: 2` = decimal-numbered.
- `anchor_id` + `position` — `"before"` | `"after"` to insert relative to an existing element.
- `action: "delete"` — delete the element with this id.

**Table element (`type: "table"`) fields**:
- `id` (optional) — modify an existing table.
- `header` (required) — column labels (becomes the first row).
- `rows` (required) — data rows as a 2-D string array, e.g. `[["metric", "95%"], ["score", "88%"]]`.
- `anchor_id` + `position` — relative insertion.
- `action: "delete"`.

**Key behavioral rules**:
1. Always specify `style` for every paragraph (`Heading1/2/3` for section titles, `Normal` for body).
2. Omitted fields on modify = preserve original — this is the safe default for "edit just one thing".
3. When you supply `runs`, they replace the entire run list. To bold a single word you must echo the other runs' text.
4. Always `read_office_file` first — otherwise ids won't line up and edits will land in the wrong paragraph.

## Long-document incremental generation
For documents expected to have **~2000+ characters**, build incrementally instead of all at once to keep each tool call manageable.

1. `create_word_doc` with `title="..."` to create the file header.
2. For each section, call `create_word_doc` with `append: true` and the new `elements[]` (~1500-2000 characters of text per chunk).
3. The first section must include a `Heading1` paragraph.
4. Repeat for subsequent sections.

## read_office_file (Word)
For .docx inputs returns:
- `text_content` — full text as a string.
- `elements` — structured element array, each paragraph / table with a stable `id` (used by `create_word_doc`).

## inspect_office (Word)
For .docx files use `format="docx", mode="info"`. Returns paragraph count, table count, word count, whether headers / footers / images / styles exist — does NOT return body content. **Use before opening a large file to gauge its size.**
- Parameters: `path`, `format="docx"`, `mode="info"`.

## compare_word_docs
Compare two .docx files; return a structured diff (`added[]` / `removed[]` / `modified[]` + summary string).
