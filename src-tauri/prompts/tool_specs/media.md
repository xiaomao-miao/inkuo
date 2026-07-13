# 媒体文件工具（read_image / read_pdf）

用于让 AI 直接消费工作区里的图片与 PDF，避免先要求用户在编辑器里"打开看一下"。

## read_image

读取工作区里的图片文件并以 `data:` URL 形式返回，附带 MIME 与大小元数据。

**支持的扩展**：`png`、`jpg`、`jpeg`、`gif`、`webp`、`bmp`、`ico`、`avif`、`tif`、`tiff`、`svg`。

**输入参数**：
- `path` (必填) — 图片的绝对路径。

**返回结构 (JSON)**：
```jsonc
{
  "path": "C:/workspace/shot.png",
  "name": "shot.png",
  "size": 184320,
  "mime": "image/png",
  "data_url": "data:image/png;base64,iVBORw0KGgo...",
  "note": "Attach data_url as an image_url content part for multimodal models."
}
```

**适用场景**：
- 用户问"这个截图里有什么"。
- 用户希望 AI 描述一张示意图、流程图、UI 截图或参考图。
- 视觉差异比较（例如渲染前后）。

**注意事项**：
- 单张最大 20 MB。更大的图片请先用外部工具压缩或缩放。
- 返回的 `data_url` 较大，但只有一种情况会真正送进模型：当上游模型支持视觉输入时由 agent 运行时把 `data_url` 转成 `image_url` 内容段。否则它会被原样回显给用户，作为工具调用的字符串结果。

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
