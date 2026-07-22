# Tool spec: `create_pptx_animation`

The `create_pptx_animation` tool packs a list of `.svg` files into a `.pptx` with **full animation support**. Every shape remains editable in PowerPoint / Keynote / WPS. This tool extends `create_pptx` with an animation layer.

Load this spec via `get_tool_help(category="pptx")` whenever the user asks for animated slides, "带动画的PPT", "animated presentation", or when they want transitions.

---

## 1. Output contract

- Writes a `.pptx` at `output_path`.
- Every slide is a native `<p:sp>` / `<p:cxnSp>` (fully editable).
- `<p:timing>` elements injected per slide with animations.
- `<p:transition>` elements per slide.
- Returns: `file_path`, `slide_count`, `animation_count`, `byte_size`.

---

## 2. Arguments reference

| Argument | Type | Required | Notes |
|---|---|---|---|
| `svg_paths` | string[] | ✓ | Absolute paths to `.svg` files. Order preserved — n-th SVG = n-th slide. |
| `output_path` | string | ✓ | Absolute workspace path ending in `.pptx`. |
| `title` | string | ✗ | Deck title stamped into `docProps/core.xml`. |
| `slide_animations` | array | ✗ | Per-slide animation specs. |
| `transition` | object | ✗ | Default slide transition for all slides. |
| `transition_speed` | string | ✗ | `"slow"` / `"med"` (default) / `"fast"`. |

### AnimationSpec schema

```json
{
  "shape": "@all | @first | @last | <index> | <name-pattern>",
  "effect": "fadeIn | flyIn | zoom | bounce | pulse | spin | fadeOut | flyOut | toggle",
  "duration_ms": 500,
  "delay_ms": 0,
  "trigger": "onclick | afterprev | withprev",
  "direction": "l | r | t | b | tl | tr | bl | br",
  "zoom_scale": 0.0
}
```

### SlideAnimationSpec schema

```json
{
  "slide_index": 0,
  "animations": [/* AnimationSpec[] */],
  "transition": { "transition_type": "fade", "direction": "r", "speed": "med" },
  "transition_speed": "med"
}
```

### TransitionEffect schema

```json
{
  "transition_type": "fade | push | wipe | cover | reveal | blind | split | checker | diamond | plus | circular | comb | crawl | fly | spiral | flash | zoom | pan | fadeThroughColor | none",
  "direction": "l | r | t | b",
  "color": "#RRGGBB",
  "speed": "slow | med | fast"
}
```

---

## 3. Animation effect catalogue

### Entrance effects

| Effect | Description | Default duration |
|---|---|---|
| `fadeIn` | Shape fades in (opacity 0→1) | 500ms |
| `flyIn` | Shape flies in from direction | 500ms |
| `zoom` | Shape zooms in from scale | 500ms |
| `bounce` | Zoom with 50% starting scale | 500ms |

### Emphasis effects

| Effect | Description | Default duration |
|---|---|---|
| `pulse` | Shape pulses (scale up and back) | 500ms |
| `spin` | Shape rotates | 500ms |

### Exit effects

| Effect | Description | Default duration |
|---|---|---|
| `fadeOut` | Shape fades out (opacity 1→0) | 500ms |
| `flyOut` | Shape flies out in direction | 500ms |

### Special effects

| Effect | Description | Default duration |
|---|---|---|
| `toggle` / `set` | Instant property change | — |

---

## 4. Trigger types

| Trigger | Behaviour |
|---|---|
| `onclick` (default) | Click to start this animation |
| `afterprev` | Start automatically after previous animation ends |
| `withprev` | Start simultaneously with previous animation |

---

## 5. Slide transition catalogue

| Type | Description | Direction |
|---|---|---|
| `fade` | Cross-fade (default) | — |
| `push` | Slide pushes old slide | l/r/t/b |
| `wipe` | Wipe reveal | l/r/t/b |
| `cover` | New slide covers old | l/r/t/b |
| `reveal` | Old slide reveals new | l/r/t/b |
| `blind` | Venetian blinds | l/r/t/b |
| `split` | Split open/close | l/r/t/b |
| `checker` | Checkerboard | l/r/t/b |
| `diamond` | Diamond wipe | — |
| `plus` | Plus wipe | — |
| `circular` | Circular wipe | l/r |
| `comb` | Comb teeth | l/r/t/b |
| `crawl` | Crawl in from edge | l/r/t/b |
| `fly` | Fly in | l/r/t/b |
| `spiral` | Spiral wipe | — |
| `flash` | Flash | — |
| `zoom` | Zoom in | — |
| `pan` | Pan | l/r/t/b |
| `fadeThroughColor` | Fade through color | — |
| `none` | No transition | — |

---

## 6. Common recipes

### 6.1 Fade-in each shape on click

```json
{
  "svg_paths": ["/workspace/slide1.svg"],
  "output_path": "/workspace/deck.pptx",
  "slide_animations": [
    {
      "slide_index": 0,
      "animations": [
        { "shape": "@all", "effect": "fadeIn", "trigger": "onclick", "duration_ms": 300 }
      ]
    }
  ]
}
```

### 6.2 Staggered entrance — shapes appear one by one

```json
{
  "svg_paths": ["/workspace/slide1.svg"],
  "output_path": "/workspace/deck.pptx",
  "slide_animations": [
    {
      "slide_index": 0,
      "animations": [
        { "shape": "@first", "effect": "flyIn", "direction": "b", "trigger": "onclick", "duration_ms": 400 },
        { "shape": "@all", "effect": "fadeIn", "trigger": "afterprev", "duration_ms": 300 }
      ]
    }
  ]
}
```

### 6.3 Slide transition — fade between slides

```json
{
  "svg_paths": ["/workspace/slide1.svg", "/workspace/slide2.svg"],
  "output_path": "/workspace/deck.pptx",
  "transition": { "transition_type": "fade" },
  "transition_speed": "med"
}
```

### 6.4 Fly-in with fade and push transition

```json
{
  "svg_paths": ["/workspace/slide1.svg"],
  "output_path": "/workspace/deck.pptx",
  "slide_animations": [
    {
      "slide_index": 0,
      "animations": [
        { "shape": "@first", "effect": "flyIn", "direction": "r", "duration_ms": 600, "trigger": "onclick" },
        { "shape": "@all", "effect": "fadeIn", "trigger": "withprev", "duration_ms": 400 }
      ],
      "transition": { "transition_type": "push", "direction": "l" },
      "transition_speed": "fast"
    }
  ]
}
```

---

## 7. SVG `<animate>` auto-conversion

SVG `<animate>` tags are automatically parsed and converted to OOXML animations:

```xml
<rect id="box" fill="blue" ...>
  <animate attributeName="opacity" from="0" to="1" dur="1s" begin="0s"/>
</rect>
```

This becomes a `fadeIn` animation on the rectangle, 1000ms duration.

Supported attributes: `opacity`, `visibility`, `display`, `fill-opacity`.

---

## 8. Failure modes

| Failure | Recovery |
|---|---|
| Output path not `.pptx` | Change extension |
| Empty `svg_paths` | Add at least one SVG |
| Animation targets wrong shape | Use `@first` / `@last` / index |
| PowerPoint shows no animation | Check shape IDs — use `inspect_office` to verify |
