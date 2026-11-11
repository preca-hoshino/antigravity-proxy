# Antigravity Proxy

Antigravity Proxy 是一个将 Google Cloud Code 内部 API（Antigravity）桥接为标准 OpenAI 与 Gemini Native 协议的反向代理服务。

本项目基于 TypeScript 开源实现 [Kazuki-0147/Antigravity-Proxy](https://github.com/Kazuki-0147/Antigravity-Proxy) 重构，使用 Rust 编写。

## 架构与支持协议

- **OpenAI 兼容端点**
  - `/v1/chat/completions`（流式与非流式）
  - `/v1/models`
  - 支持通过 `reasoning_effort` (`low`, `medium`, `high`) 或模型名称后缀控制思考推理档位。
- **Gemini 原生端点**
  - `/v1beta/models/{model}:{action}`
  - `/v1/models/{model}:{action}`
- **管理与状态接口**
  - 内嵌 Web 管理面板（默认端口 `8317`）
  - Server-Sent Events (`/api/sse`) 状态与日志推送
  - 多端点配额同步与轮询机制

## 支持模型映射

服务自动处理内部 Sandbox 通道与 CLI 生产通道的分流：

| 客户端请求模型名 | 内部路由通道 | 思考支持 | 说明 |
| :--- | :--- | :--- | :--- |
| `gemini-3.8-flash` | Sandbox | low / medium / high | 默认等级为 high |
| `gemini-3.7-flash` | Sandbox | low / medium / high | 默认等级为 high |
| `gemini-3.1-pro` | Sandbox | low / high | - |
| `gemini-4-argon` | Sandbox | 默认开启 | Google 旗舰推理模型 |
| `claude-sonnet-4-6` | Sandbox | thinking | 映射至 `claude-sonnet-4-6-thinking` |
| `claude-opus-4-6` | Sandbox | thinking | 映射至 `claude-opus-4-6-thinking` |
| `gemini-2.5-pro` | CLI | 预算制 | 需相应访问权限 |
| `gemini-2.5-flash` | CLI | 预算制 | 需相应访问权限 |

## 构建与运行

### 依赖环境
- Rust 1.80 或更高版本
- Cargo

### 编译
```bash
cargo build --release
```
编译产物位于 `target/release/antigravity-proxy`（Windows 下为 `antigravity-proxy.exe`）。

### 账号配置
初始化配置文件与账号文件：
```bash
cp antigravity-accounts.example.json antigravity-accounts.json
```
向 `antigravity-accounts.json` 中填入凭据后，可执行刷新命令同步配额：
```bash
./target/release/antigravity-proxy account refresh
```

### 启动服务
```bash
# 默认监听 127.0.0.1:8317
./target/release/antigravity-proxy serve

# 指定监听地址与端口
./target/release/antigravity-proxy serve --host 0.0.0.0 --port 8317
```

服务启动后，可通过浏览器访问对应地址查看管理面板。

### 命令行工具

```text
Usage: antigravity-proxy [COMMAND]

Commands:
  serve    启动代理服务与管理控制台
  account  账号与配额管理（list, add, remove, refresh, reset）
  config   查看或更新本地配置
  test     连通性自检与流式调用测试
  help     显示帮助信息
```

## 致谢

本项目参考了 [Kazuki-0147/Antigravity-Proxy](https://github.com/Kazuki-0147/Antigravity-Proxy) 的逆向工程与协议分析成果，特此向原作者及相关贡献者致谢。

## 许可证

本项目基于 [MIT 许可证](LICENSE) 分发。
