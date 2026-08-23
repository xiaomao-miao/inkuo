# Tool spec: `create_pptx`

`create_pptx` packages ordered, self-contained SVG slides into a PowerPoint file and runs structured static presentation-quality inspection. Supported text/basic geometry remains editable; raster media and complex geometry follow the limits below. Use it only after the deck's audience, purpose, intended outcome, narrative, visual tokens, and slide claims have been decided.

## Contract

- One SVG becomes one slide; `svg_paths` order is preserved.
- The first SVG defines the presentation canvas. Use the same `viewBox` on every page; default to `0 0 1280 720` (16:9).
- Supported text and basic geometry become native DrawingML. Raster media remains a picture object; gradients are flattened and complex-path editing may vary by host application.
- The complete package is written to a unique sibling temp file, flushed and synced, then safely activated at `output_path`; if activation fails, the previous deck is preserved/restored. Parent directories are created.
- POSIX activation uses an atomic rename. Windows uses a backup-and-restore fallback because standard rename cannot replace an existing file; it is recoverable on reported errors but is not a strict single-system-call atomic replacement across a process/power crash.
- Speaker notes are real PPT notes parts, not visible slide text.
- The return value contains deck-level and per-slide **static source QA**. It does not render PowerPoint, assess every claim's provenance, or guarantee identical host-app rendering. A file can be written with `status: "needs_revision"`; this is a preserved draft and its tool result is marked as an error so it cannot be mistaken for successful completion.

## Arguments

| Argument | Type | Required | Notes |
| --- | --- | --- | --- |
| `svg_paths` | `string[]` | yes | 1–200 absolute workspace `.svg` paths. Each file ≤12 MiB; combined input ≤96 MiB. |
| `output_path` | `string` | yes | Absolute workspace path ending in `.pptx`. |
| `title` | `string` | no | Core-properties title. |
| `speaker_notes` | `string[]` | no | Empty, or exactly one note per slide. External claims and assets go under `[Sources]`. |

Example:

```json
{
  "svg_paths": [
    "/workspace/deck/01-title.svg",
    "/workspace/deck/02-evidence.svg"
  ],
  "output_path": "/workspace/launch-plan.pptx",
  "title": "Launch Plan",
  "speaker_notes": [
    "[Sources]\n- User-provided brief; no external source.",
    "[Sources]\n- https://example.com/report — metric and chart data"
  ]
}
```

## Authoring requirements

Use a consistent authoring-token system but vary adjacent layout silhouettes. The generated package uses one shared blank PowerPoint master/layout; `create_pptx` does not synthesize reusable custom master variants from the SVGs. Each slide still needs one primary claim and a takeaway-style title. Avoid card grids, dashboards, pills, fake buttons, navigation chrome, and repeated component-library patterns.

SVG fonts are converted at `0.75 PowerPoint pt per SVG px`. Unless a supplied template explicitly overrides them, use at least:

| Role | SVG size | PowerPoint size |
| --- | ---: | ---: |
| Deck title | 67 px | 50 pt |
| Slide title | 47 px | 35 pt |
| Subheading/callout | 32 px | 24 pt |
| Body | 22 px | 16 pt |

Keep title text on one line. Shorten copy or change the layout before reducing type. Use equal safe margins, normally 72–96 px on a 1280×720 canvas.

## Supported SVG

| SVG | PowerPoint result |
| --- | --- |
| `rect`, `circle`, `ellipse` | Editable preset shape |
| `line` | Editable connector |
| `polyline`, `polygon`, `path` | Native custom geometry; editing fidelity varies for complex paths |
| `text`, `tspan` | Editable text runs |
| simple `g` translate/uniform-scale | Coordinates baked into children |
| inline PNG/JPEG `image` | Embedded editable picture object |
| linear/radial gradient | Portable solid fill using the first stop |

`create_svg` can resolve an `asset://...` image reference into an inline data URL before this tool reads the SVG. Images are currently placed into their declared frame, so pre-crop the source to the frame aspect ratio; QA reports `media_stretched` when intrinsic and frame ratios differ.

Unsupported or risky elements such as `use`, `foreignObject`, filter, mask, clip path, pattern, and switch are skipped and reported in `skipped_elements`; scripts are never an authoring mechanism. Use presentation attributes (`fill`, `stroke`, `stroke-width`) instead of CSS-dependent effects. Keep all gradient definitions inside the SVG that references them.

## Speaker-note sources

Every externally sourced non-trivial claim and every external/generated media asset requires a literal block in that slide's note:

```text
[Sources]
- <direct URL or local file> — what was used
- Generated: <model/tool and brief> — asset provenance
```

For a page with no external source, use `[Sources]\n- User-provided content; no external source.` Do not put private production notes on the visible canvas.

The inspector can mechanically require `[Sources]` only when it detects embedded media. It cannot determine whether arbitrary visible prose is an externally sourced claim; claim-level provenance remains the author's responsibility even when static QA passes.

## QA result

The result includes:

```json
{
  "status": "ok | needs_revision",
  "completion_gate": {
    "blocking": false,
    "next_action": "Run render_office_preview and inspect actual slide pixels before final handoff."
  },
  "visual_verification": {
    "status": "not_run"
  },
  "quality": {
    "passed": true,
    "error_count": 0,
    "warning_count": 0,
    "issues": [
      {
        "severity": "error | warning",
        "code": "title_too_small",
        "message": "...",
        "slide": 2
      }
    ]
  },
  "slides": [
    {
      "index": 1,
      "shape_count": 8,
      "skipped_elements": [],
      "quality_issues": []
    }
  ]
}
```

The static inspector checks or heuristically predicts:

- 16:9/default and cross-slide canvas consistency;
- deck/slide title and body minimum sizes;
- predicted one-line title fit;
- text/shape overflow and probable text overlap;
- unresolved placeholders;
- media aspect distortion/crop review and media source notes;
- skipped SVG elements;
- three consecutive pages with the same layout silhouette.

QA is conservative and complements, rather than replaces, rendered full-slide visual review. A zero-error result means only that these static checks passed. Fix every error. Inspect each warning and either revise it or keep it for a concrete reason. Then overwrite only the affected SVGs and call `create_pptx` again with the same full list and output path until `quality.error_count` is zero.

Once `status` is `ok`, call `render_office_preview` with the generated `.pptx`. It produces at most 8 real slide PNGs per call and queues them into the next multimodal model iteration; continue from `next_start_page` until all slides have been viewed. Inspect clipping, overlap, title wrapping, placeholder text, font substitution, legibility, hierarchy, alignment, spacing, contrast, and media crop. Revise source SVGs, rebuild, and rerender when any issue appears. If the renderer reports that it is unavailable, never ask the user to install dependencies and never claim visual verification; report the static-only result and residual host-rendering risk.

## Recovery

| Problem | Action |
| --- | --- |
| Wrong extension or empty SVG list | Correct the arguments; never use `write_file` for PPTX. |
| More than 200 slides / oversized SVG input | Split the deck or optimize embedded raster assets; do not bypass the 12 MiB-per-slide / 96 MiB-total safety limits. |
| Note count differs from slide count | Supply exactly one note per slide or omit notes entirely. |
| `title_may_wrap` | Shorten the takeaway title or change layout; do not shrink below 35 pt. |
| `text_overlap` / overflow | Reposition or reduce copy, regenerate that SVG, and rerun. |
| `media_stretched` | Crop/export the image to the intended frame ratio before embedding. |
| `missing_media_sources` | Add a `[Sources]` block to that slide's speaker note. |
| Repeated silhouette | Switch one page to a content-appropriate layout family. |
| Skipped element | Re-author it with supported primitives or a pre-cropped PNG/JPEG. |
