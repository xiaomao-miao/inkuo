# Tool spec: `create_svg`

The `create_svg` tool lets the agent author a beautiful, **self-contained** SVG file and write it to the workspace. SVG is the **final output** — no rasterization, no font installation, no browser render. Any modern viewer (browsers, image viewers, docx inserter, in-app preview) renders it losslessly at any size.

Load this spec via `get_tool_help(category="svg")` whenever the user asks for an icon, illustration, banner, badge, diagram-like-but-not-mermaid image, decorative graphic, or any vector artwork. Also load it when the user pastes a brief like "draw me…" / "make a logo…" / "我想要一个…" / "生成一个…的图片".

---

## 1. Output contract

The tool writes a single file at `output_path` (must end in `.svg`). After a successful call:

- The file is on disk and the in-app viewer auto-opens it.
- The tool returns a JSON blob describing the write (file_path, byte_size, viewBox).
- The frontend can build a `data:image/svg+xml;base64,...` URL from the tool's output if it wants to inline-preview in the chat card.

The `svg_source` argument must be a **complete standalone document** — it will be written verbatim. If you forget the `<?xml ?>` prolog or the `xmlns` attribute the tool will reject the call with a clear error.

---

## 2. Mandatory scaffolding

Every SVG must start with this skeleton (the `?xml` prolog is optional but recommended; the `xmlns` is **required**):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 <W> <H>" width="<W>" height="<H>">
  <!-- your artwork -->
</svg>
```

Pick `viewBox` and `width`/`height` yourself. There is **no required size** — choose what fits the content. Common starting points:

| Use case              | viewBox (logical units) | Notes                                            |
| --------------------- | ----------------------- | ------------------------------------------------ |
| Square icon           | `0 0 64 64`             | Tight around the art, no whitespace padding      |
| Rectangular icon      | `0 0 24 24`             | Material-design style                            |
| Banner / hero         | `0 0 1280 320`          | 4:1 ratio                                        |
| Card / illustration   | `0 0 400 300`           | 4:3 ratio                                        |
| Social card           | `0 0 1200 630`          | Open Graph / Twitter card                        |
| Diagram (non-mermaid) | `0 0 800 600`           | Larger to fit labels                             |

The `viewBox` is the intrinsic coordinate system; the on-screen size is controlled by the viewer's CSS. Never lock a `width`/`height` that prevents scaling — leave both as plain numbers without `px` so the renderer can scale fluidly.

---

## 3. Aesthetics guidelines

The user is paying for *good* SVG, not just *valid* SVG. Apply these in every call:

### 3.1 Palette (3–5 colors)

Pick a small, harmonious palette and **declare it as CSS variables at the top of the SVG** so the same artwork is easy to re-theme later. Example:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300">
  <style>
    .bg     { fill: #FAF7F2; }   /* warm off-white background */
    .ink    { fill: #1F2933; }   /* primary stroke / text */
    .accent { fill: #7C5CFF; }   /* brand accent */
    .muted  { fill: #9AA5B1; }   /* secondary stroke */
  </style>
  <rect class="bg" width="400" height="300"/>
  <!-- … -->
</svg>
```

When in doubt, choose a palette from a known good source:

- **Vibrant & modern**: `#7C5CFF #4CC9F0 #F72585 #FFD166 #06D6A0`
- **Calm & editorial**: `#1F2933 #3E4C59 #E4E7EB #F5F7FA #9AA5B1`
- **Warm & natural**: `#FAF7F2 #2E2A26 #B5651D #D4A373 #588157`

Never use pure `#000` or `#FFF` as a fill — they look harsh on screen and clash with both light and dark app themes. Reach for `#1F2933` and `#FAF7F2` instead.

### 3.2 Geometry & strokes

- **Stroke widths**: pick 2 to 4 distinct widths (e.g. `1, 2, 4`) and stick to them. Avoid 1px hairlines — they alias badly on hi-DPI displays; use 1.5 or 2.
- **`stroke-linecap="round"` and `stroke-linejoin="round"`** for any non-rectilinear shape. Square ends look unfinished.
- **`fill="none"`** is your friend for outlined icons. Combine with `stroke="currentColor"` (declared via `<style>`) to make the icon theme-aware.
- **Avoid bitmaps**: never `<image href="…">` to a raster. Use `<path>` / `<rect>` / `<circle>` / `<line>` / `<polygon>` for everything.

### 3.3 Typography

- **Always use real `<text>` elements** with `font-family` set, not paths-shaped-like-letters. Modern renderers pick the right font; paths-as-text look terrible when re-themed.
- Default font stack that works everywhere: `font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"`.
- For monospace: `font-family="ui-monospace, 'SF Mono', Menlo, Consolas, monospace"`.
- `text-anchor="middle"` for centered labels, `"start"` / `"end"` for aligned.
- Pick `font-size` in **viewBox units**, not screen pixels — `font-size="24"` reads consistently at any rendered size.
- `font-weight` matters: `400` for body, `600` for headings, `700` for emphasis. Don't go above `800`.

### 3.4 Composition

- **Generous whitespace.** If the artwork fills 100% of the viewBox, it looks cramped. Leave 8–12% margin on every side, or center the artwork in a larger viewBox.
- **Optical alignment, not mathematical alignment.** A circle is "centered" when the negative space around it looks balanced, not when its `(cx, cy)` is exactly `viewBox / 2`. For round / organic shapes nudge them up & left by 1–2 units to compensate for visual weight.
- **Hierarchy** through size, weight, and color — not through sheer quantity of detail. A clean two-element composition beats a busy ten-element one.
- **Use `<g>` to group related elements** so a single transform / style change re-positions them coherently.

### 3.5 Forbidden patterns

The tool rejects SVGs containing any of:

- `<script>` — no scripting. Use pure declarative shapes.
- `<foreignObject>` — keeps the SVG portable.
- `xlink:href="http..."` or `href="http..."` — no external image / data references. Inline every asset.
- Inline base64 raster data longer than ~1 KB — defeats the point of vector. Use shapes.

---

## 4. Common recipes

### 4.1 Simple outlined icon (24×24, "settings" gear)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <circle cx="12" cy="12" r="3"/>
  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
</svg>
```

### 4.2 Hero illustration (1200×630 social card)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 630">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#7C5CFF"/>
      <stop offset="100%" stop-color="#4CC9F0"/>
    </linearGradient>
  </defs>
  <rect width="1200" height="630" fill="url(#bg)"/>
  <g transform="translate(120, 220)">
    <text font-family="ui-sans-serif, system-ui, sans-serif" font-size="96" font-weight="700" fill="#FAF7F2">inkuo</text>
    <text y="80" font-family="ui-sans-serif, system-ui, sans-serif" font-size="32" font-weight="400" fill="#FAF7F2" opacity="0.85">AI document editor</text>
  </g>
</svg>
```

### 4.3 Bar chart (data visualization, no scripts)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300" font-family="ui-sans-serif, system-ui, sans-serif">
  <style>
    .bar { fill: #7C5CFF; }
    .axis { stroke: #9AA5B1; stroke-width: 2; }
    .label { fill: #3E4C59; font-size: 16px; }
  </style>
  <!-- baseline -->
  <line class="axis" x1="40" y1="260" x2="380" y2="260"/>
  <!-- bars -->
  <rect class="bar" x="60"  y="160" width="40" height="100"/>
  <rect class="bar" x="120" y="80"  width="40" height="180"/>
  <rect class="bar" x="180" y="120" width="40" height="140"/>
  <rect class="bar" x="240" y="40"  width="40" height="220"/>
  <rect class="bar" x="300" y="100" width="40" height="160"/>
  <!-- x labels -->
  <text class="label" x="80"  y="285" text-anchor="middle">Q1</text>
  <text class="label" x="140" y="285" text-anchor="middle">Q2</text>
  <text class="label" x="200" y="285" text-anchor="middle">Q3</text>
  <text class="label" x="260" y="285" text-anchor="middle">Q4</text>
  <text class="label" x="320" y="285" text-anchor="middle">Q5</text>
</svg>
```

---

## 5. Workflow

1. **Read the brief carefully.** If the user wants a *flowchart / sequence / class diagram*, prefer the `render_mermaid` tool instead — it produces more accurate diagrams than hand-rolled SVG. Reserve `create_svg` for non-diagram artwork.
2. **Plan the composition** in your head (or in scratchpad) before writing the SVG. Decide viewBox, palette, and which elements go where.
3. **Author the SVG** following the rules above.
4. **Call `create_svg({ description, svg_source, output_path, aspect_ratio? })`.** The tool validates and writes. The user sees the result in the in-app viewer immediately.
5. **Tell the user what you made.** One-line summary: the file path, what it depicts, and any assumption you made (e.g. "I used a 4-color editorial palette — let me know if you want a different style").

---

## 6. Failure modes & recovery

| Failure                                          | Recovery                                                                                       |
| ------------------------------------------------ | ---------------------------------------------------------------------------------------------- |
| Tool rejects: "missing xmlns"                    | Add `xmlns="http://www.w3.org/2000/svg"` to the root `<svg>` element.                          |
| Tool rejects: "forbidden `<script>`"             | Remove the script. Replace any dynamic effect with a static `<animate>` if you need motion.    |
| Tool rejects: "external http reference"           | Inline the asset as a `<path>` / `<rect>`. SVGs are vector — there is rarely a reason to fetch. |
| User says "I wanted a PNG"                       | You have no rasterisation tool. Hand the user the SVG and ask them to export PNG (most viewers: File → Export as PNG). If the artwork is a diagram, suggest `render_mermaid` instead — but **do not write a Python script to convert**. |
| User says "I wanted a diagram"                   | Switch to `render_mermaid` — Mermaid has better layout primitives than hand-rolled SVG.       |
| SVG renders but the user dislikes the aesthetic | Re-call `create_svg` with the same path; the tool overwrites. Adjust palette / spacing / strokes. |
