# Document format conversion (svg_to_png / word_to_pdf)

The `document_converter` sub-agent wraps two "source file → target file" tools. Both are pure-Rust and offline-friendly:

| Tool           | Source       | Target    | Engine                                          |
| -------------- | ------------ | --------- | ----------------------------------------------- |
| `svg_to_png`   | `.svg`       | `.png`    | `resvg` (Skia subset, no Node/Chromium)         |
| `word_to_pdf`  | `.docx`      | `.pdf`    | `office2pdf` (Typst backend, no LibreOffice)    |

The main agent does NOT have these tools. To use them, `delegate_to({ expert: "document_converter", task: "..." })`. Markdown-to-Word conversion is not supported here — use `office_word_expert` to author `.docx` directly.

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