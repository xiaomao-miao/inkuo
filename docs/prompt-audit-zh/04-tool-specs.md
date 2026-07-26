# 04. 按需工具规范中文翻译

加载入口：`src-tauri/src/agent/prompts.rs:157-193`。模型通过 `get_tool_help(category)` 获取，内容作为 tool result 进入对话，而不是 system prompt。

> 重要实现差异：`TOOL_SPECS` 实际只注册 `general/word/excel/pptx/markdown/media/svg` 七个 category。目录中的 `pptx_animation.md` 和 `add_pptx_animation.md` 没有在该数组中注册；`get_tool_help` 的 schema 也没有列出 `pptx`。因此“9 份规范都可按需加载”并不成立。

## 4.1 General

源：`src-tauri/prompts/tool_specs/general.md:1-46`

- `read_file`：读取文本；大文件先定位，再用 offset/limit 分块。
- `write_file`：创建或完整覆盖，自动建父目录；不得写 XLSX。
- `edit_file`：精确替换，默认唯一匹配；失败时重读并复制准确文本；小改用 edit，新建/重写用 write。
- `create_dir`：递归创建目录。
- `move_file`：移动或重命名。
- `list_dir/glob`：列目录和按 glob 查找。
- `grep`：跨文件字面子串搜索，默认不区分大小写，不是正则；高级搜索交给代码专家。
- `database_search`：工作区知识库语义搜索，必须先由 UI 构建；返回片段、路径、行号和相关度。无结果时提示用户构建 KB。

## 4.2 Word

源：`src-tauri/prompts/tool_specs/word.md:1-266`

修改 Word 前几乎总要先 `read_office_file` 获取稳定 element id。

### `create_word_doc`

统一完成新建、修改、追加和删除。每次都必须传绝对 `path`；可选 title/elements/deletes/append/sections/headers/footers。title 会自动产生 Title 段落，elements 中不得再重复标题。

段落支持 id、text、style、runs、alignment、text direction、numbering、anchor 和 delete。修改时省略字段表示保留；一旦提供 runs 就完整替换。字号使用 half-points，颜色为六位 RGB。

表格有 header/rows；图片需要本地 path 与 EMU 宽高。每段应指定 style，编辑前先读 id。

### Sections / 页眉页脚 / 域代码

sections 控制分页、纸张、边距、方向、分栏、页码格式及 header/footer 引用。headers/footers 目前只支持文本。field 支持 PAGE、NUMPAGES、DATE、TIME、AUTHOR、TITLE 和自定义指令。

### 长文生成

约 2000 字符以上应增量构建：先传 path+title，再按每块 1500–2000 字符 append；每块重复 path，首节从真正的 Heading1 开始，不重复文档标题。

### 读取和比较

`read_office_file` 返回文本与元素；`inspect_office(info)` 用于大文件预检；`compare_word_docs` 返回结构化差异。

规范含三个完整 JSON 示例：竖排中文封面、带页眉页码的多 section 报告、连续分节的双栏简报。示例参数和结构保持原 JSON 语义。

## 4.3 Excel

源：`src-tauri/prompts/tool_specs/excel.md:1-88`

核心是“只读将要触碰的内容，只改已经读过的内容”。按最小范围读取，一次只做一个逻辑步骤。

`inspect_office`：

- info：sheet/cell/formula 规模。
- metadata：sheet 名、合并范围、公式地址、used range。
- range：具体区域的值、公式和样式。

推荐先 info，再 metadata，最后只钻取下一步所需 range。小工作簿确需全景时才用 `read_office_file`。

`modify_excel(path, operations)` 每次只表达一个逻辑步骤，写临时文件后原子替换，未提及单元格完全保留。操作包括改单元格、写二维区域、合并、调整行列、管理 sheet。

`create_excel` 只用于全新工作簿；现有文件会被完全覆盖，因此优先增量修改。sheet 名大小写敏感，依赖刚写数据的下一步前必须重读。

反例：为改单元格读完整工作簿；把建 sheet、写 header、公式和格式塞进一个 mega batch；用 create 更新现有文件；数据移动后不重读。

## 4.4 Markdown

源：`src-tauri/prompts/tool_specs/markdown.md:1-28`

Markdown 没有专用工具，只用 read/write/edit。长文先给不超过八行的提纲并确认，按 1500–2000 字符分章节写，完成后重读并改善过渡。第一行是 H1，合理使用 H2/H3、GFM、Markdown 链接；无用户要求不加表情。不要把整篇长文塞进 `edit_file.old_text`，frontmatter 修改应精确替换。

## 4.5 Media

源：`src-tauri/prompts/tool_specs/media.md:1-72`（原文已经是中文）

`read_image` 把图片字节放入进程内 asset registry，只把短 `asset_id/asset://` 引用返回给模型；原始字节不进入历史，下游 `create_svg` 落盘前再替换为 data URL。支持常见图像格式，单张 20 MB，id 一小时有效。目前明确**不支持视觉理解**。

`read_pdf` 按页提取文本，适合文字 PDF；扫描件可能为空。支持 max_pages，单文件 100 MB；复杂表格和版式可能丢失。

规范内有矛盾：扫描 PDF 建议“用 read_image 让视觉模型处理截图”，但同一文档又明确当前不支持多模态视觉理解。

## 4.6 SVG

源：`src-tauri/prompts/tool_specs/svg.md:1-190`

`create_svg` 生成美观、自包含、可无损缩放的 SVG。图标、插画、banner、badge、非 Mermaid 图形或矢量艺术都应加载此规范。

### 合约和骨架

output 必须 `.svg`；`svg_source` 是完整独立文档，根元素必须声明 `xmlns`，推荐 XML prolog。viewBox 和尺寸按场景自行选择：图标、banner、卡片、社交卡、图示等。

### 美学要求

使用 3–5 色协调 palette，建议通过 CSS variables 组织；避免纯黑纯白。stroke 保持少量固定宽度并用圆角端点；优先矢量 shapes。文本使用真实 `<text>` 和跨平台字体栈。留 8–12% 空白，做视觉而非机械居中，用层级而不是堆细节。

禁止 script、foreignObject、外部 HTTP 引用和长 base64 raster。

### 流程

流程图优先 Mermaid；其他图先规划 viewBox、palette、构图，再生成并调用工具，最后告知路径、内容和假设。失败时修正 xmlns、脚本、外部引用或审美参数。

规范包含 gear icon、社交卡和柱状图三个完整 SVG 示例。

## 4.7 PPTX

源：`src-tauri/prompts/tool_specs/pptx.md:1-117`

`create_pptx` 把 SVG 转为原生 OOXML shape，保持 PowerPoint/Keynote/WPS 可编辑。输入按 `svg_paths` 顺序成为 slides，输出 `.pptx`，可带 title。

每个 SVG 应有 xmlns/viewBox；首个 SVG 决定整个 deck 的 slide size。支持 rect/circle/ellipse/line/polyline/polygon/path/text，以及有限 translate+uniform scale。image/use/foreignObject/filter/mask/clipPath/pattern/switch 会被丢弃。

渐变只取第一个 stop 作为纯色；跨文件 defs 会变 noFill。shape 上 inline style 暂不解析，应使用 presentation attributes。常见失败包括扩展名错误、空路径、跳过元素和不可见 shape。

工作流：先确认用户要“可编辑形状”而非栅格保真；验证 SVG；检查不支持元素；一次创建完整 deck；调整时改 SVG 后覆盖重建。

## 4.8 `create_pptx_animation`（存在文件但未注册）

源：`src-tauri/prompts/tool_specs/pptx_animation.md:1-232`

该规范描述从 SVG 创建带动画 PPTX：每个 shape 仍可编辑，slide 写入 timing 和 transition。参数包含 `slide_animations`、全局 transition 和 speed。

动画效果：fadeIn/flyIn/zoom/bounce、pulse/spin、fadeOut/flyOut、toggle/set；trigger 为 onclick/afterprev/withprev。过渡包含 fade、push、wipe、cover、reveal、blind、split、checker、diamond 等二十种。还声称可自动把 SVG `<animate>` 的 opacity/visibility/display/fill-opacity 转为 OOXML 动画。

问题：它没有出现在 `TOOL_SPECS`，也没有独立 category；当前模型无法通过已公布的 `get_tool_help` 稳定取得这份说明。

## 4.9 `add_pptx_animation`（存在文件但未注册）

源：`src-tauri/prompts/tool_specs/add_pptx_animation.md:1-168`

该规范描述为现有 PPTX 注入动画并写到新文件，原文件不改。参数为 input/output、按页 slides、全局 transition/speed。效果和 trigger 与创建动画版一致。实现步骤是读取 ZIP、定位 slide XML、删除旧 timing/transition、注入新节点并写出。

同样未注册到 `TOOL_SPECS`；同时主 Agent/PowerPoint Expert 的工具清单也未显示这两个动画工具，功能即使存在也难被模型发现。

## 工具规范层面的观察

- Word、SVG、PPTX 规格非常长，作为 tool result 会永久增加会话负担。
- v1 Agent 提示词中 Tier 2 描述自相矛盾（"你必须加载 spec"、"你没有这些工具"、"直接委派"同时存在）。v2（见 `02-core-modes.md§2.1`）已统一为"只为识别名字存在"，三层矛盾已收敛为单句。
- `get_tool_help` 的短描述漏掉 `pptx`，而实际 registry 又包含 `pptx`；动画两份完全漏注册。
- 工具 spec 与 profile prompt 大量重复，维护时极易一边更新、另一边过时。
- 主 Agent 工具表在 PROFILES 注册中只列 14 个工具，与 `agent.slim.md` 表格完全一致；建议同步在 `prompts.rs` 注册文件的注释里添加"主代理 14 个工具 = `agent.slim.md §1.1` 表格的完整名单"的提示，避免双向漂移。
