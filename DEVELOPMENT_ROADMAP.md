# inkuo 开发路线图 & 时间表

> 基于 2026-06-02 技术评审，假设全职开发（每天 8 小时）。

---

## 一、当前状态

- **代码量**：Rust 后端 ~25 个源文件，React 前端 ~10 个组件
- **核心架构**：✅ Agent Loop、工具注册、流式 streaming、Diff 引擎均已跑通
- **工具集**：❌ 仅 8 个工具（缺 Web 搜索、Shell、Git 等）
- **RAG**：❌ 伪嵌入，无法用于生产
- **产品化**：❌ 无 README/官网/定价/案例
- **融资准备**：❌ 无 pitch deck / 演示视频

---

## 二、优先级矩阵

| 优先级 | 类型 | 方向 | 理由 |
|:---:|------|------|------|
| P0 | 核心 | **工具集扩展** | Agent 无法查资料 = 无法完成论文/方案 |
| P0 | 核心 | **真实 Embedding** | RAG 是摆设 = 上下文检索无效 |
| P0 | 产品 | **产品 README + Demo 视频** | 没有 demo = 无法融资 |
| P1 | 核心 | **工作流模式（Paper/Project）** | 降低使用门槛，让 AI 有章法 |
| P1 | 产品 | **Pitch Deck** | 融资的必备材料 |
| P2 | 核心 | **Todo 任务管理工具** | 多章节/多文件协同缺少主线 |
| P2 | 产品 | **官网 / Landing Page** | 面向投资人和用户的门面 |
| P2 | 体验 | **UI 打磨** | 非技术用户的可解释性 |
| P3 | 可选 | **Git 工具** | 进阶项目管理的需求 |

---

## 三、开发阶段详细任务

---

### Phase 0：环境准备 & 已知问题修复（0.5 天）

- [ ] `src-tauri/src/agent/tools/file_tools.rs` 第 10 行：`pub fn definition()` 是裸函数，与 struct 方法 `pub fn definition()` 同名编译会报错，需改为 `read_file_definition()` 或合并到 impl 块
- [ ] 确认当前 build 能通过：`cargo build --release` 无 warning
- [ ] 确认前端 dev server 能正常启动
- [ ] 整理 `.gitignore`，确保不泄露 API key

---

### Phase 1：工具集扩展（3 天）

#### 1.1 Web 搜索工具（1.5 天）

**目标**：让 Agent 能自主搜索最新资料来支撑论文/方案写作。

```
工具名：web_search
参数：query (string), num_results (integer, default 5)
返回：标题 + URL + 摘要 的列表
```

**实现方案**（二选一）：

| 方案 | API | 优点 | 缺点 | 工期 |
|------|-----|------|------|:---:|
| A | Tavily Search API | 专为 AI 设计，有结构化输出 | 每日有限额 | 1 天 |
| B | DuckDuckGo HTML 爬取 | 免费，无 API Key | 需解析 HTML，较脆弱 | 2 天 |

**推荐方案 A**，理由：专注核心功能，Tavily 有免费 tier（1000次/月）。

**文件改动**：
- `src-tauri/src/agent/tools/` 新建 `web_tools.rs`
- `src-tauri/src/agent/tools/mod.rs` 注册工具
- `src-tauri/prompts/agent.md` 工具列表添加 web_search
- `src-tauri/src/commands_agent.rs` 工具注册处更新

#### 1.2 Shell 执行工具（0.5 天）

**目标**：让 Agent 能编译、运行测试、验证代码。

```
工具名：run_command
参数：command (string, required), cwd (string, optional)
返回：stdout + stderr + exit_code
```

- 用 `std::process::Command`（同步足够）
- 需要 workspace 路径验证
- 限制危险命令（`rm -rf /`, `mkfs` 等黑名单）

**文件改动**：
- `src-tauri/src/agent/tools/` 新建 `shell_tools.rs`
- `src-tauri/src/agent/tools/mod.rs` 注册

#### 1.3 文件批次操作工具（0.5 天）

**目标**：支持一次性创建多个文件（如论文的各个章节）。

```
工具名：batch_write_files
参数：files (array of {path, content}, required)
```

- 遍历数组，原子性写入
- 任一失败返回错误（不污染成功文件）

#### 1.4 Todo 任务管理工具（0.5 天）

**目标**：让 Agent 在长任务中记录进度。

```
工具名：create_todo
参数：title (string), description (string, optional), status (string: "pending"|"in_progress"|"done")

工具名：list_todos
参数：无

工具名：update_todo
参数：todo_id (string), status (string)
```

- 存储在 `$WORKSPACE/.inkuo/todos.json`
- Agent 在执行复杂任务时用 todo 跟踪进度

---

### Phase 2：RAG 系统升级（1 天）

#### 2.1 接入真实 Embedding（0.5 天）

**问题**：`generate_pseudo_embedding()` 用的是词频 hash，无法做语义检索。

**方案**：接入 OpenAI `text-embedding-3-small`（1536 维，有免费额度）。

**改动**：

```rust
// src-tauri/src/rag.rs
async fn generate_embedding(text: &str) -> Vec<f32> {
    // 调用 OpenAI Embeddings API
    let client = reqwest::Client::new();
    let resp = client.post("https://api.openai.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": "text-embedding-3-small",
            "input": text
        }))
        .send()
        .await
        .unwrap();
    // 解析 response.data[0].embedding
}
```

- 将 embedding 维度从 64 改为 1536
- cosine_similarity 保持不变

#### 2.2 RAG 自动检索注入（0.5 天）

**目标**：Agent 在执行任务时，自动把相关知识库片段注入到 context。

**实现**：
- 在 `commands_agent.rs` 的 `ai_agent_stream` 里，发送前做一次 RAG 搜索
- 把 top-k 结果拼到 system prompt 末尾或 user message 的 context 部分
- `k=5`，每个 chunk 最多 500 字符

---

### Phase 3：工作流模式（1 天）

#### 3.1 Paper Mode / Project Mode Preset

**目标**：让 AI 扮演专业论文写作者或架构师，有结构化的工作流程。

新增 prompt 文件：

**`src-tauri/prompts/paper.md`**：
```
# inkuo AI - 论文写作模式

## 角色
你是一位专业的学术论文写作者。你熟悉 IEEE/ACM/Nature 等格式规范。

## 工作流程（严格遵循）
1. **确认主题**：用户给主题 → 确认大纲
2. **制定大纲**：先产出目录（Abstract + N Sections + References）
3. **逐章节写作**：每个章节单独输出，用户确认后再推进
4. **整合修订**：所有章节写完后做整体润色
5. **格式检查**：检查参考文献格式、图表编号、摘要长度

## 输出格式
每个章节输出：
{
  "chapter": "2",
  "title": "...",
  "content": "...",
  "word_count": N,
  "next_action": "..."
}
```

**`src-tauri/prompts/project.md`**：
```
# inkuo AI - 项目方案模式

## 角色
你是一位经验丰富的技术架构师和产品经理。

## 工作流程
1. **需求分析**：理解背景和目标
2. **技术选型**：给出推荐技术栈和理由
3. **系统设计**：模块划分、接口设计、数据流
4. **实施计划**：分阶段，里程碑，验收标准
5. **风险评估**：Top-3 风险 + 缓解方案

## 约束
- 方案要具体可执行
- 每步产出要可验证
- 代码示例要可直接运行
```

#### 3.2 模式切换 UI

- 在 `ChatInput.tsx` 或 `ChatHeader.tsx` 添加 "Paper" / "Project" 模式按钮
- 模式切换时更新 system prompt
- 前端 store 新增 `presetMode: 'general' | 'paper' | 'project'`

---

### Phase 4：产品化文档 & Demo（1.5 天）

#### 4.1 产品 README（0.5 天）

**`README.md`**（用户面向，非技术）：

```markdown
# inkuo - AI 驱动的本地文档编辑器

**让 AI 真正动手写，而不是只动嘴。**

inkuo 是一个本地优先的 AI 文档编辑器。不同于普通的 AI 对话工具，inkuo 的 AI Agent 可以：
- 读取、搜索、创建、修改你的文件
- 自主规划论文大纲并逐章节写作
- 基于你的项目代码生成完整方案
- 全程在本地运行，数据永不离开你的电脑

## 核心场景

### 论文写作
[场景描述 + 截图]

### 项目方案
[场景描述 + 截图]

### 代码开发
[场景描述 + 截图]

## 安装

## 定价

## 案例
```

#### 4.2 Demo 视频脚本（0.5 天）

录制 3 分钟视频，脚本：

```
00:00 - 00:15  开场：问题引入
  "写一篇论文需要什么？资料、思路、结构、反复修改..."
  "今天我让 inkuo AI 来帮我写一篇完整的项目方案。"

00:15 - 00:45  场景设定
  打开 inkuo，切换到 Project Mode
  输入：「帮我设计一个中小型团队的 OKR 管理系统的技术方案」

00:45 - 01:30  AI 自主工作
  录制 AI 自动创建文件、搜索参考资料、执行命令的完整过程
  展示工具调用卡片 + Diff 确认

01:30 - 02:00  结果展示
  打开生成的项目方案文档
  展示目录结构、章节内容

02:00 - 02:30  差异化
  对比通用 AI 对话：普通 AI 只能给建议
  inkuo AI 能直接生成文件、验证代码、修改确认

02:30 - 03:00  收尾
  "inkuo — 让 AI 真正动手做。"
  官网 / GitHub 链接
```

#### 4.3 Pitch Deck 初稿（0.5 天）

10-15 页 PPT：

1. **封面**：logo + 一句话定位
2. **问题**：知识工作者写论文/方案的痛苦
3. **现有方案**：通用 AI 的局限（只能给建议，不能动手做事）
4. **解决方案**：inkuo 的核心价值主张
5. **产品演示**：3 张截图
6. **技术架构**：本地优先 + AI Agent + 隐私
7. **商业模式**： Freemium → Pro 订阅
8. **市场规模**：知识工作工具市场
9. **竞争分析**：vs Cursor / Claude Code / Notion AI
10. **团队**：创始成员背景
11. **融资需求**：融资金额 + 用途（6-12 个月 runway）
12. **联系方式**

---

### Phase 5：打磨 & 锦上添花（1 天）

#### 5.1 错误处理增强（0.5 天）

- Web 搜索失败：优雅降级，返回"搜索服务暂不可用"
- Shell 命令超时：设置 30s 超时，中途可取消
- API Key 缺失：清晰的错误提示，引导去设置面板配置

#### 5.2 UI 可解释性（0.5 天）

- 工具调用时：显示当前正在执行的操作（"正在搜索资料...", "正在写入文件..."）
- Diff 预览：添加"AI 做了什么"的中文总结（而非纯 diff）
- 工作进度：展示"第 3/8 章节"等进度指示

---

## 四、时间总表

| 天数 | 阶段 | 任务 | 交付物 |
|:---:|:---:|------|------|
| **Day 0.5** | Phase 0 | 环境准备 + 编译修复 | `cargo build` 通过 |
| **Day 1-2** | Phase 1.1 | Web 搜索工具（Tavily API） | `web_search` 工具可用 |
| **Day 2** | Phase 1.2 | Shell 执行工具 | `run_command` 工具可用 |
| **Day 2-3** | Phase 1.3 | 批次写文件 + Todo 工具 | 4 个新工具可用 |
| **Day 3-4** | Phase 2 | RAG 升级（真实 Embedding） | 语义搜索生效 |
| **Day 4-5** | Phase 3 | Paper/Project Mode | 两种工作流可用 |
| **Day 5-6** | Phase 4.1 | 产品 README | 外部可读的产品文档 |
| **Day 6-6.5** | Phase 4.2 | Demo 视频 | 3 分钟演示视频 |
| **Day 6.5-7** | Phase 4.3 | Pitch Deck 初稿 | 融资材料 |
| **Day 7-8** | Phase 5 | 错误处理 + UI 打磨 | 产品体验提升 |

> **注**：Phase 5 是缓冲时间，如果前面的任务提前完成，用来打磨；如果超时，砍掉 Phase 5 的 UI 部分，保留错误处理。

---

## 五、融资时间表（并行于开发）

| 时间点 | 事件 |
|--------|------|
| **Day 1** | 搭建 product landing page（纯静态页，notion.so / vercel） |
| **Day 5** | Demo 视频完成 |
| **Day 7** | Pitch Deck 完成 |
| **Day 8** | 接触第一个天使投资人 |
| **Week 2-3** | 迭代 pitch，收集反馈 |
| **Week 4** | 完成 pre-seed 融资目标（目标 $50K-$200K） |

---

## 六、里程碑

```
[Day 0.5]  ✅ 编译通过，内部可测试
[Day 5]    ✅ 工具集完整 + RAG 升级 = "可演示的 MVP"
[Day 7]    ✅ README + Demo 视频 + Pitch Deck = "可融资状态"
[Week 4]   ✅ 拿到 pre-seed
[Week 6]   ✅ 官网 + 案例研究 + 公开发布
[Month 3]   ✅ Product-Market Fit 验证
[Month 6]   ✅ Seed Round（目标 $500K-$1M）
```

---

## 七、每个里程碑的验收标准

### Milestone 1：可测试 MVP（Day 0.5）
- [ ] `cargo build --release` 零错误
- [ ] `npm run dev` 前端正常启动
- [ ] 能打开编辑器并创建文件

### Milestone 2：可演示 MVP（Day 5）
- [ ] AI Agent 能自主搜索资料并写一个 5 页的 markdown 文档
- [ ] Web 搜索、Shell、Todo 工具均可用
- [ ] Diff 确认流程流畅
- [ ] Paper Mode 能产出一篇有结构的论文大纲

### Milestone 3：可融资状态（Day 7）
- [ ] 产品 README 让非技术人员能理解价值
- [ ] 3 分钟 Demo 视频能清晰传达核心价值
- [ ] Pitch Deck 逻辑清晰、数据有据

### Milestone 4：Pre-seed 完成（Week 4）
- [ ] 获得至少 1 个天使投资人意向
- [ ] 资金到账
- [ ] 下一阶段 roadmap 确定

---

## 八、风险与备选方案

| 风险 | 概率 | 影响 | 应对 |
|------|:---:|:---:|------|
| Tavily API 超额/不可用 | 中 | 中 | 降级到 DuckDuckGo（多花 0.5 天） |
| Embedding API 成本超预期 | 低 | 中 | 限制每天索引次数 + 本地缓存 |
| Demo 视频效果不好 | 中 | 高 | 多录几版，聚焦最惊艳的 3 个功能 |
| 投资人反馈需要大改 | 高 | 中 | 保持敏捷，每轮反馈都迭代 pitch |

---

*最后更新：2026-06-02*
