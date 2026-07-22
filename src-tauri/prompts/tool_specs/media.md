# 媒体文件工具（read_image / read_pdf）

用于让 AI 直接消费工作区里的图片与 PDF，避免先要求用户在编辑器里"打开看一下"。

## read_image

读取工作区里的图片文件并把字节**存入进程内的 asset registry**，返回一个短小的 `asset_id`，附带元数据。

**关键设计**：图片的原始字节**永远不会进入对话历史**——它们走的是一条单独的侧通道（`asset_registry`）。AI 拿到 `asset_id` 后，可以在 `create_svg` 等下游工具的占位符里用 `asset://<asset_id>` 引用这张图；这些工具在落盘前一刻会把引用替换为真正的 `data:` URL。这样即使嵌入一张 1 MB 的 PNG 也不会撑爆上下文窗口。

**支持的扩展**：`png`、`jpg`、`jpeg`、`gif`、`webp`、`bmp`、`ico`、`avif`、`tif`、`tiff`、`svg`。

**输入参数**：
- `path` (必填) — 图片的绝对路径。

**返回结构 (JSON)**：
```jsonc
{
  "asset_id": "asset-1a2b3c4d",
  "asset_ref": "asset://asset-1a2b3c4d",
  "path": "C:/workspace/shot.png",
  "name": "shot.png",
  "size_bytes": 184320,
  "size_human": "180.0 KB",
  "mime": "image/png",
  "ext": "png",
  "usage": "Embed via `<image href=\"asset://<asset_id>\" .../>` in create_svg ..."
}
```

**典型用法**：
1. 调用 `read_image({ path: "shot.png" })`，得到 `asset_id = "asset-1a2b3c4d"`。
2. 调用 `create_svg({ svg_source: '<svg ...><image href="asset://asset-1a2b3c4d" .../></svg>', output_path: "out.svg" })`。
3. `create_svg` 写入磁盘前会把 `asset://asset-1a2b3c4d` 替换为真正的 base64 数据 URL，得到的 SVG 完全自包含。

**适用场景**：
- 用户希望把 PNG / JPEG 嵌入到生成的 SVG / PPTX 中。
- 任何"读取图片并把它的内容塞进生成产物"的工作流。

**注意事项**：
- 单张最大 20 MB。更大的图片请先用外部工具压缩或缩放。
- `asset_id` 在进程内有效 1 小时；超过后再次引用会得到清晰的 `unknown or expired` 错误提示，此时重新调用 `read_image` 即可。
- 当前**不**支持多模态视觉理解（如"告诉我这张截图里有什么"）。要支持视觉理解需要在前端层把图片直接追加到用户消息的 `image_url` 内容段，那是单独的集成。

## read_pdf

读取工作区里的 PDF 文件并按页提取嵌入文本。**最佳效果是文字型 PDF**；扫描件（无文本层）会返回空页面，请改用 `read_image` 让具备视觉能力的模型处理每一页的截图。

**输入参数**：
- `path` (必填) — PDF 绝对路径。
- `max_pages` (可选) — 只读取前 N 页。

**返回结构 (JSON)**：
```jsonc
{
  "path": "C:/workspace/report.pdf",
  "size": 5242880,
  "page_count": 32,
  "pages": ["第一章 引言\n...", "1.1 背景\n...", "..."],
  "truncated": false
}
```

**适用场景**：
- 用户希望 AI 总结一份 PDF、抽取要点、回答 PDF 里的问题。
- 长文档（>几十页）请配合 `max_pages` 使用，避免一次性超出上下文窗口。

**注意事项**：
- 单个 PDF 上限 100 MB。
- 表格、二进制图表、嵌入字体在文本抽取中可能丢失排版；如对版式敏感，可先用 `read_image` 提取关键页。
- PDF 解析由 `pdf-extract`（纯 Rust）完成，无外部 native 依赖。
