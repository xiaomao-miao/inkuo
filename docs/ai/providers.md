# AI：Provider 适配（OpenAI-compatible / Ollama / Official AI）

## 1. 目标
- 用统一接口调用不同 provider。
- 统一支持：流式、取消、超时、重试。
- 统一输出协议：`summary/content/rules_applied`。

## 2. Provider 类型

### 2.1 OpenAI-compatible
- 兼容 Chat Completions（或 Responses，视实现选择）。
- 支持 DeepSeek 等兼容网关。

### 2.2 Ollama
- 本地 HTTP API。
- 需要适配其流式协议与模型列表。

### 2.3 Official AI（会员）
- 通过 inkuo 网关鉴权。
- 支持配额与模型路由。

## 3. 通用参数
- model
- temperature
- max_tokens
- json/structured output 开关
- streaming

## 4. 错误处理（MUST）
- 网络失败：可重试（指数退避）
- 超时：提示并允许继续编辑
- 结构化解析失败：进入协议降级（见 protocol 文档）
