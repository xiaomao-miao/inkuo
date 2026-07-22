# Tool spec: `add_pptx_animation`

The `add_pptx_animation` tool adds animations to an **existing** `.pptx` file. It reads the PPTX, injects `<p:timing>` and `<p:transition>` elements into specified slides, and writes a new PPTX.

Load this spec via `get_tool_help(category="pptx")` whenever the user wants to add animations to an existing PPTX, "给PPT加动画", "add animation to presentation", etc.

---

## 1. Output contract

- Reads the `input_pptx`.
- Writes a **new** `output_pptx` with animations injected.
- Original `input_pptx` is not modified.
- Returns: `file_path`, `slide_count`, `animation_count`, `byte_size`.

---

## 2. Arguments reference

| Argument | Type | Required | Notes |
|---|---|---|---|
| `input_pptx` | string | ✓ | Absolute path to existing `.pptx`. |
| `output_pptx` | string | ✓ | Absolute path for output `.pptx`. Must end in `.pptx`. |
| `slides` | array | ✓ | Per-slide animation specs. |
| `transition` | object | ✗ | Default slide transition for all slides. |
| `transition_speed` | string | ✗ | `"slow"` / `"med"` (default) / `"fast"`. |

### SlideAnimationSpec schema

```json
{
  "slide_index": 0,
  "animations": [/* AnimationSpec[] */],
  "transition": { "transition_type": "fade", "direction": "r" },
  "transition_speed": "med"
}
```

### AnimationSpec schema

```json
{
  "shape": "@all | @first | @last | <0-based-index>",
  "effect": "fadeIn | flyIn | zoom | bounce | pulse | spin | fadeOut | flyOut | toggle",
  "duration_ms": 500,
  "delay_ms": 0,
  "trigger": "onclick | afterprev | withprev",
  "direction": "l | r | t | b | tl | tr | bl | br"
}
```

---

## 3. Animation effects (same as `create_pptx_animation`)

### Entrance (click to reveal)
- `fadeIn` — opacity 0→1
- `flyIn` — fly from direction
- `zoom` — scale from `zoom_scale` to 100%
- `bounce` — zoom from 50%

### Emphasis
- `pulse` — scale pulse
- `spin` — rotation

### Exit
- `fadeOut` — opacity 1→0
- `flyOut` — fly out in direction

### Special
- `toggle` / `set` — instant property change

### Triggers
- `onclick` (default) — user clicks to trigger
- `afterprev` — auto-plays after previous ends
- `withprev` — plays simultaneously with previous

---

## 4. Slide transitions

Same 20 types as `create_pptx_animation`: `fade`, `push`, `wipe`, `cover`, `reveal`, `blind`, `split`, `checker`, `diamond`, `plus`, `circular`, `comb`, `crawl`, `fly`, `spiral`, `flash`, `zoom`, `pan`, `fadeThroughColor`, `none`.

---

## 5. Common recipes

### 5.1 Add fade-in animation to all shapes on slide 0

```json
{
  "input_pptx": "/workspace/existing.pptx",
  "output_pptx": "/workspace/animated.pptx",
  "slides": [
    {
      "slide_index": 0,
      "animations": [
        { "shape": "@all", "effect": "fadeIn", "trigger": "onclick", "duration_ms": 300 }
      ]
    }
  ]
}
```

### 5.2 Staggered animations — shapes appear one at a time

```json
{
  "input_pptx": "/workspace/deck.pptx",
  "output_pptx": "/workspace/animated_deck.pptx",
  "slides": [
    {
      "slide_index": 0,
      "animations": [
        { "shape": "@first", "effect": "flyIn", "direction": "b", "trigger": "onclick", "duration_ms": 400 },
        { "shape": "@all", "effect": "fadeIn", "trigger": "afterprev", "duration_ms": 300, "delay_ms": 0 }
      ]
    },
    {
      "slide_index": 1,
      "animations": [
        { "shape": "@all", "effect": "fadeIn", "trigger": "onclick", "duration_ms": 500 }
      ],
      "transition": { "transition_type": "fade" }
    }
  ]
}
```

### 5.3 Apply transitions to all slides

```json
{
  "input_pptx": "/workspace/deck.pptx",
  "output_pptx": "/workspace/deck_transitioned.pptx",
  "slides": [
    { "slide_index": 0, "animations": [] },
    { "slide_index": 1, "animations": [] },
    { "slide_index": 2, "animations": [] }
  ],
  "transition": { "transition_type": "push", "direction": "l" },
  "transition_speed": "med"
}
```

---

## 6. How it works

1. Reads the ZIP entries from `input_pptx`.
2. For each matching slide (by `slide_index`), parses the raw slide XML.
3. Removes any existing `<p:timing>` and `<p:transition>` elements.
4. Injects new `<p:timing>` (animations) and `<p:transition>` (slide transition).
5. Writes all entries to `output_pptx`.

The shape count is auto-detected from `<p:sp>` / `<p:cxnSp>` elements in the slide XML, so you don't need to know shape IDs in advance.

---

## 7. Failure modes

| Failure | Recovery |
|---|---|
| `output_pptx` doesn't end in `.pptx` | Change extension |
| `input_pptx` doesn't exist | Check path with `list_dir` |
| Slide index out of range | Check slide count first |
| PowerPoint shows no animation | Shape IDs may not match — use index selectors |
