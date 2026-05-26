# 安全：API Key 存储与调用链路

## 1. 目标
- Key 仅在本机安全存储中保存。
- 前端 JS 不接触明文 key。
- 自带 Key 模式下，AI 请求不经过 inkuo 服务器。

## 2. 存储
- macOS：Keychain
- Windows：Credential Manager
- Linux：secret-service（GNOME Keyring / KWallet 等）

## 3. 调用链路（MUST）
- UI → IPC → Rust AI Proxy → Provider
- Rust 从 keyring 取 key 后发起请求。

## 4. 安全措施
- 设置页必须展示数据流路径说明。
- 支持一键清除本机 key。
- 对可疑配置（例如把 key 填进普通配置文件）给出警告。
