# Tool spec: `create_pptx`

The `create_pptx` tool packs a list of `.svg` files into a single `.pptx` presentation in which **every shape remains editable** in PowerPoint / Keynote / WPS. SVG geometry is converted to native OOXML shapes (`<p:sp>` / `<p:cxnSp>` / `<a:custGeom>`) rather than rasterised to a bitmap inside an `<p:pic>`. Load this spec via `get_tool_help(category="pptx")` whenever the user asks for a slide deck, a PowerPoint, a `.pptx` file, a 演示, 幻灯片, deck, presentation, or "把这几张图做成 PPT".

---

## 1. Output contract

The tool writes a single file at `output_path` (must end in `.pptx`). After a successful call:

- The file is on disk and PowerPoint / Keynote / WPS will open it without conversion.
- Every shape on every slide is a native `<p:sp>` (rect / ellipse / connector / custom path / text box), so the user can recolour, resize, edit text, or re-arrange layers inside PowerPoint.
- The tool returns a JSON blob describing the write (`file_path`, `slide_count`, per-slide `shape_count`, per-slide `skipped_elements`).
- The frontend can open the file directly via the OS file association.

---

## 2. Arguments reference

| Argument      | Type           | Required | Notes                                                                                            |
| ------------- | -------------- | -------- | ------------------------------------------------------------------------------------------------ |
| `svg_paths`   | string[]       | ✓        | Absolute paths to `.svg` files. **Order is preserved** — n-th SVG becomes the n-th slide. At least one entry. |
| `output_path` | string         | ✓        | Absolute workspace path ending in `.pptx`. Parent directories auto-created.                       |
| `title`       | string         | ✗        | Deck title, stamped into `docProps/core.xml` and shown in PowerPoint's "Title" field.              |

---

## 3. Mandatory scaffolding (per input SVG)

Each input SVG becomes one slide. To look its best in the deck, the SVG should:

- Declare `xmlns="http://www.w3.org/2000/svg"` so PowerPoint's XML parser accepts it.
- Declare a `viewBox="0 0 W H"` (or at least `width` + `height`) — the deck is 16:9 (13.333" × 7.5"), and the SVG is fit-to-slide preserving its aspect ratio. **Without a `viewBox` we fall back to width/height, which often produces off-centre results.**
- Use the supported SVG subset listed in §4.

If you want a different aspect ratio per slide, use a matching `viewBox` in the source SVG (e.g. `viewBox="0 0 1600 900"` for 16:9, `viewBox="0 0 1920 1080"` for 16:9 widescreen at higher precision).

---

## 4. Supported SVG subset

Every element below becomes a native PowerPoint shape — fully editable:

| SVG element                  | OOXML target                                  | Editable in PPT?                                                            |
| ---------------------------- | --------------------------------------------- | --------------------------------------------------------------------------- |
| `<rect>`                     | `<p:sp>` preset geometry `rect` / `roundRect` | ✓ (resize, recolour, edit corner radius)                                    |
| `<circle>` / `<ellipse>`     | `<p:sp>` preset geometry `ellipse`            | ✓                                                                           |
| `<line>`                     | `<p:cxnSp>` connector                         | ✓ (re-stroke, change line style)                                            |
| `<polyline>` / `<polygon>`   | `<p:sp>` with `<a:custGeom>` path             | ✓ (geometry locked at the original vertex count; user can recolour + move) |
| `<path>`                     | `<p:sp>` with `<a:custGeom>` custom path      | ✓ (PowerPoint preserves the path; recolour + edit vertex handles)           |
| `<text>`                     | `<p:sp>` with `<p:txBody>`                    | ✓ (fully editable text; per-run bold / italic / underline preserved)        |
| `<g transform="translate(x y) scale(s)">` | applied to children coordinates    | ✓ (the children render in their new position on the slide)                  |

### Forbidden / unsupported (silently dropped)

| Element                          | Why                                                            |
| -------------------------------- | -------------------------------------------------------------- |
| `<image>` / `<use>` / `<foreignObject>` | These embed other documents / raster images; we can't keep them editable in v1. |
| `<filter>` / `<mask>` / `<clipPath>`     | DrawingML filter stacks are not portable; we'd need per-renderer fallbacks. |
| `<pattern>`                      | Pattern fills require a separate theme part.                   |
| `<switch>`                       | SVG runtime content-negotiation; not meaningful in PPT.        |

### `<linearGradient>` / `<radialGradient>` — first-stop fallback

Shapes filled with `url(#id)` references **are** supported: the parser walks the `<defs>` block, captures the first `<stop>`'s colour (and `stop-opacity`), and emits that as the shape's `<a:solidFill>`. Subsequent stops in the same gradient are intentionally ignored — we don't try to recreate the colour ramp in DrawingML because the ramp doesn't render portably across PowerPoint / Keynote / WPS. Practical consequences:

- A 2-stop `bg: #1a1a2e → #0f3460` gradient resolves to `#1a1a2e` everywhere the SVG uses `fill="url(#bg)"`. The deck looks slightly less rich than the source SVG; that's the trade-off for portability.
- If the gradient lives in a different SVG (no matching `<linearGradient id="…">` block in the source file), the reference degrades to `<a:noFill/>` and the shape becomes invisible. The `skipped_elements` field doesn't list this case, so always tell the user to expect solid-colour fills when they ask for gradients.
- Both the standalone `stop-color="…"` attribute and the inline `style="stop-color:…"` form are honoured.

If the user actually wants the full gradient ramp to land in PPT, the only escape hatch today is `render_mermaid`-style raster output — recommend that instead.

CSS-style presentation attributes are honoured (`fill="..."`, `stroke="..."`, `stroke-width="..."`, `fill-opacity="..."`, `opacity="..."`). Inline `style="…"` declarations on shape elements are NOT parsed in v1 — the source SVG should prefer presentation attributes. (Inline `style="…"` IS parsed for `<stop>` because that's where `create_svg` and friends put gradient colours.)

---

## 5. Common recipes

### 5.1 Two-slide deck from two icons

```json
{
  "svg_paths": ["/workspace/slide1.svg", "/workspace/slide2.svg"],
  "output_path": "/workspace/deck.pptx",
  "title": "Product Pitch"
}
```

### 5.3 SVG uses `<g transform="translate(x, y) scale(s)">`

No special handling needed — `create_pptx` walks the SVG, applies the parent transform to every child coordinate (including `<path>` `d` attributes), and emits the result as a native OOXML shape. So `<g transform="translate(100, 50)"><path d="M 10 20 L 30 40 Z"/></g>` becomes a path with points `(110, 70) → (130, 90)` on the slide.

Only `translate` + uniform `scale` are honoured. `rotate`, `skewX/Y`, and `matrix(...)` are silently ignored — the shape will still draw, just untransformed.

---

## 6. Failure modes & recovery

| Failure                                              | Recovery                                                                                  |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Tool rejects: `output_path` doesn't end in `.pptx`   | Change the extension.                                                                     |
| Tool rejects: empty `svg_paths`                      | Add at least one SVG path.                                                                |
| Tool rejects: a `svg_paths` entry doesn't end in `.svg` | Fix the path.                                                                            |
| Tool returns: per-slide `skipped_elements` non-empty | Tell the user which elements were dropped (e.g. "slide 1 lost 1 `<image>` element"). Suggest re-authoring if the dropped elements matter. |
| PowerPoint opens the deck but a shape is invisible   | The source SVG explicitly used `fill="none"` for that shape — re-author with a solid fill, OR the SVG referenced a gradient whose `<defs>` block is in a different file (we degrade to noFill in that case — re-author or merge the gradients into the SVG itself). |
| User wanted rasterised images for fidelity           | This tool does NOT do that — switch to `render_mermaid` (PNG) for raster output.         |

---

## 7. Workflow

1. **Confirm the user wants an editable deck, not a flat image sequence.** The single biggest reason for rework is the user expecting raster fidelity and getting editable shapes instead.
2. **Verify every input SVG exists** (`list_dir` / `glob`).
3. **Check the SVGs are compatible** with §4's supported subset. If unsure, peek at the source with `read_file` — `<image>` / `<use>` / `<foreignObject>` are the common surprises. Gradients are supported but resolve to the first stop's colour, so manage user expectations accordingly.
4. **Call `create_pptx`** with the full `svg_paths[]` in the order the user wants.
5. **Tell the user what was made** — file path, slide count, any skipped elements.
6. **If the user wants tweaks**, re-author the source SVG and re-call `create_pptx` with the same `output_path` (the tool overwrites).