# Antigravity Proxy (Rust Edition)

> 🚀 高性能 Google Antigravity & Cloud Code 内部 API 反向代理与 CLI 控制中心 (Rust 重构版本)。  
> 将 Google 强大的 Gemini 4 Argon、Gemini 3.8/3.7 Flash、Claude 5.5 / 4.6 等前沿模型转化为标准 OpenAI / Gemini 格式接口。

---

## 🌟 致谢与原项目

本项目是基于优秀的 TypeScript 开源项目 [AntigravityProxy (by Kazuki-0147)](https://github.com/Kazuki-0147/Antigravity-Proxy) 使用 **Rust** 进行的高性能完全重写。

感谢原作者 [@Kazuki-0147](https://github.com/Kazuki-0147) 及社区贡献者在逆向分析 Google Cloud Code 内部协议、设备指纹伪装与调度逻辑上做出的先驱性工作！

---

## ✨ 核心特性

- **极致性能与低资源占用**：由 Rust + Tokio + Axum 驱动，常驻内存仅数十 MB，极低延迟。
- **全格式兼容**：
  - OpenAI 兼容接口 (`/v1/chat/completions`, `/v1/models`)
  - Gemini Native 原生接口 (`/v1beta/models/...`)
  - 支持 DeepSeek / OpenAI 风格思考控制参数 (`reasoning_effort: "low" | "medium" | "high"`)
- **前沿模型支持**：
  - **Gemini 4 Argon**（Google 最新旗舰推理模型）
  - **Gemini 3.8 / 3.7 Flash**（支持 low / medium / high 三档深度思考调度）
  - **Claude 5.5 / 4.6 系列**（Sonnet / Opus 深度推理思考）
- **官方最新配额同步**：
  - 完美对齐官方三大额度分类：**Gemini Models (5h & Weekly)**、**Claude/GPT**、**Gemini 4 Argon**
  - 多端点探测与自动兜底，精准反映当前账号真实剩余比例与重置倒计时。
- **内置 WebUI 与精准指标遥测**：
  - 实时 SSE 状态同步与仪表盘。
  - 请求详细遥测：总耗时、首字延迟 (TTFT)、Decode 纯生成速率 (tok/s)、Prompt 缓存命中率 (Cache Hit %)、Thinking Token 明细统计。

---

## 🚀 快速开始

### 1. 编译构建

```bash
cargo build --release
```

产物将生成在 `target/release/antigravity-proxy.exe`。

### 2. 账号授权与配额刷新

```bash
# 查看帮助
./target/release/antigravity-proxy.exe --help

# 导入/刷新账号配额
./target/release/antigravity-proxy.exe account refresh
```

### 3. 启动服务

```bash
# 启动反代服务（默认端口 8317，包含内置 WebUI）
./target/release/antigravity-proxy.exe serve --port 8317
```

浏览器打开 [http://127.0.0.1:8317/](http://127.0.0.1:8317/) 即可进入管理控制台。

---

## 📄 开源许可证

本项目基于 [MIT 许可证](LICENSE) 开源。
