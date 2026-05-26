# UI：主题系统（Cursor-like + Accent 可配置）

## 1. 目标
- 默认观感 Cursor-like（中性色阶固定，强调色突出交互焦点）。
- 用户主要只配置 Accent，避免破坏可读性。
- 支持主题导入/导出（JSON），可在团队/工作区共享。

## 2. 内置主题
- `cursor-dark`（默认，蓝紫冷色 Accent）
- `cursor-light`
- `high-contrast-dark`
- `high-contrast-light`

## 3. 可配置项（默认开放）
- `accent`（hex）

## 4. 白名单微调项（可选开放）
- diff 色相：added/removed hue
- selection alpha
- focus ring strength

## 5. Design Tokens（建议集合）
- `--bg`, `--fg`, `--panel`, `--border`
- `--accent`, `--accent-contrast`
- `--focus-ring`
- `--diff-added`, `--diff-removed`
- `--selection`

## 6. 主题 JSON（规范）
- 文件结构示例：

```json
{
  "name": "Cursor Dark (Custom Accent)",
  "base": "cursor-dark",
  "accent": "#7C5CFF",
  "options": {
    "diffHueAdded": 145,
    "diffHueRemoved": 5,
    "selectionAlpha": 0.25,
    "focusRingStrength": 1.0
  }
}
```

## 7. 校验规则（MUST）
- `base` 必须为内置基线。
- 仅允许覆盖 `accent` 与 `options` 白名单字段。
- 导入失败必须给出可理解错误（字段/范围）。
