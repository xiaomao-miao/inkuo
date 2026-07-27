# Document format conversion (svg_to_png / md_to_word / word_to_pdf)

The `document_converter` sub-agent wraps three "source file → target file" tools. All three are pure-Rust and offline-friendly:

| Tool           | Source       | Target    | Engine                                          |
| -------------- | ------------ | --------- | ----------------------------------------------- |
| `svg_to_png`   | `.svg`       | `.png`    | `resvg` (Skia subset, no Node/Chromium)         |
| `md_to_word`   | `.md` text   | `.docx`   | `pulldown-cmark` → in-house `WordDocument` writer |
| `word_to_pdf`  | `.docx`      | `.pdf`    | `office2pdf` (Typst backend, no LibreOffice)    |

The main agent does NOT have these tools. To use them, `delegate_to({ expert: "document_converter", task: "..." })`.

## svg_to_png

Rasterize a single `.svg` file to a `.png` file. Pure-Rust, no Node.js / Chromium / system font cache dependency.

**Required**: `input_path` (absolute), `output_path` (absolute).

**Optional**:
- `max_width` (int) — caps the rendered width; SVG is scaled down (aspect ratio preserved) when its intrinsic width exceeds the cap.
- `max_height` (int) — same, for height.
- `background` (string) — CSS color painted behind the SVG. Accepts `#RGB`, `#RRGGBB`, `#RRGGBBAA`, or named colors (`white`, `black`, `red`, `green`, `blue`, `yellow`, `gray`/`grey`, `lightgray`/`lightgrey`). Default: `transparent`.

**Returns**: `{"input_path", "output_path", "bytes"}`.

**Pitfalls**:
- For Mermaid diagrams use `render_mermaid` via `flowchart_expert` instead — same engine, but already returns the rendered SVG; rasterizing it through `svg_to_png` only adds an unnecessary round-trip.
- If the SVG references remote images via `http(s)://`, the renderer falls back silently. Use `create_svg` with embedded base64 images, or pre-download the assets.
- The output PNG is at the SVG's intrinsic size unless capped by `max_width` / `max_height`.

## md_to_word

Convert Markdown to a Word `.docx`. The path is `pulldown-cmark` (CommonMark + GFM tables / strikethrough / task lists / smart punctuation) → in-house `WordDocument` model → the same OOXML writer that `create_word_doc` uses for everything else.

**Input source — exactly one of**:
- `input_path` (string) — absolute path to a `.md` / `.markdown` file.
- `markdown` (string) — inline Markdown source. Use this when the Markdown is already in your context (e.g. you just `read_file`'d it).

**Required**: `output_path` (absolute, ends in `.docx`).

**Optional**: `title` (string) — emitted as the first Heading 1 paragraph.

**Returns**: `{"output_path", "bytes", "paragraphs", "tables"}`.

**Coverage**:
- Headings (`#`–`######`), paragraphs, emphasis (`*`/`_`), strong (`**`/`__`), strikethrough (`~~`), inline code (backticks), fenced code blocks (with language), blockquotes, horizontal rules, ordered / unordered lists, nested lists, GFM task lists (`- [ ]` / `- [x]`), GFM tables (with header row).
- Smart punctuation (en/em dashes, curly quotes) when `ENABLE_SMART_PUNCTUATION` is on (default).
- Inline links render as plain text in v1 — the in-house writer does not yet emit `<w:hyperlink>` runs. Tell the user to open the doc and re-add links if needed.
- Images are not embedded; they appear as `[image]` markers in the output.

**Pitfalls**:
- The converter is intentionally not exhaustive. Anything outside the subset above degrades to plain text (the docx still opens without errors).
- Footnote references render as `[*]`. A future patch can wire the in-house footnote machinery.

## word_to_pdf

Convert a Word `.docx` to a `.pdf`. Pure-Rust via `office2pdf` (Typst backend) — no LibreOffice, no Chromium, no Docker.

**Required**: `input_path` (absolute, `.docx`), `output_path` (absolute, `.pdf`).

**Optional**:
- `paper_size` (`a4` / `letter` / `legal`) — default: `a4`.
- `landscape` (`true` / `false`) — default: `false`.

**Returns**: `{"input_path", "output_path", "bytes"}`.

**Coverage**: text formatting (bold / italic / underline / color), tables, images, headers / footers, page setup (size, orientation, margins).

**Pitfalls**:
- Complex layouts (multi-column sections, complex tables, embedded charts) may not render pixel-identical to Microsoft Word. The output is "good enough" for sharing / printing; for archival-grade fidelity, export from Word directly.
- The conversion is single-shot — there is no incremental "edit the PDF" path. For changes, edit the source `.docx` and re-run.