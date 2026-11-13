# http-sse：HTTP/SSE 实时推送

[返回项目总览](../README.md)

基于 Axum 和 Tokio 的 Server-Sent Events（SSE）示例，演示通过 HTTP 长连接持续推送数据，包括模拟系统监控指标和模拟 AI 流式文本输出。项目内置浏览器测试面板，使用原生 `EventSource` 接收事件。

监控指标由程序模拟生成，聊天回复使用预设文本，无需配置系统监控服务或模型 API。

## 目录结构

```text
http-sse/
├── README.md
├── Cargo.toml
├── Cargo.lock
└── src/
    └── main.rs        # HTTP 路由、SSE 事件流和内置 HTML 测试面板
```

## 环境准备

- 安装 Rust stable 工具链（包含 `rustc`、`cargo` 和 `rustfmt`），建议使用最新稳定版。
- 本案例使用 Rust Edition 2024。
- 首次构建需要下载 Cargo 依赖。
- 确保本机 `3000` 端口未被占用。

## 启动服务

在 `http-sse` 目录下执行：

```bash
cargo run
```

服务监听 `127.0.0.1:3000`。打开 [浏览器测试面板](http://127.0.0.1:3000/)，可发送提示词查看流式文本，或建立、断开指标推送连接。在服务终端按 `Ctrl+C` 停止程序。

## 接口说明

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/` | 内置 HTML 测试面板 |
| GET | `/sse/metrics` | 持续推送模拟监控指标 |
| GET | `/sse/chat?prompt=...` | 推送预设聊天文本，`prompt` 可选 |

### 实时监控指标

在另一个终端执行，`-N` 用于关闭 curl 输出缓冲，便于观察实时事件：

```bash
curl -N http://127.0.0.1:3000/sse/metrics
```

服务每隔约 1 秒发送一条 `metric` 事件，包含递增的 `id`、`retry: 3000` 和 JSON 格式的 `data`：

| 字段 | 含义 |
| --- | --- |
| `seq` | 当前事件序号，默认从 1 开始 |
| `cpu_usage` | 模拟 CPU 使用率，单位为百分比 |
| `memory_mb` | 模拟内存使用量，单位为 MB |
| `timestamp` | 生成指标时的 Unix 时间戳，单位为秒 |

接口读取 `Last-Event-ID` 请求头，可手动验证从序号 11 开始推送：

```bash
curl -N -H 'Last-Event-ID: 10' http://127.0.0.1:3000/sse/metrics
```

未提供该请求头或值无法解析为无符号整数时，从序号 1 开始。这里仅演示重连后的序号延续，不存储或补发断线期间的历史事件；每条连接独立生成数据。浏览器面板手动断开后再次建立连接会创建新的 `EventSource`，从头开始接收。

序号达到 `u64::MAX` 后结束当前流，避免整数溢出和重复 ID；若请求头已是该最大值，则返回空事件流。要重新开始，可关闭旧连接并创建不带该请求头的新连接。

事件中的 `retry: 3000` 设置浏览器自动重连等待时间为 3 秒。接口还配置了 15 秒空闲心跳，使用 `keep-alive` 注释保活；正常每秒推送指标时通常不会出现心跳。指标流持续发送，可按 `Ctrl+C` 停止 curl。

### 模拟流式对话

```bash
curl -N -G --data-urlencode 'prompt=What is SSE?' http://127.0.0.1:3000/sse/chat
```

不提供 `prompt` 时，默认使用 `What is HTTP/SSE?`；提供时最多回显前 100 个字符。程序按约 150 毫秒的间隔发送预设文本片段：

| 事件 | 数据 | 说明 |
| --- | --- | --- |
| `token` | JSON，包含 `token` 和 `index` | 文本片段及从 0 开始的索引，事件 `id` 与索引对应 |
| `done` | 字符串 `[DONE]` | 文本发送完成，随后结束流 |

该接口不处理 `Last-Event-ID`，重新请求会从头输出。内置面板收到 `done` 后主动关闭 `EventSource`。自行编写客户端时，应监听命名事件 `token` 和 `done`，并在完成后关闭连接。

## 日志与开发检查

程序日志使用英文，可通过 `RUST_LOG` 调整过滤级别。在 `http-sse` 目录下执行：

```bash
RUST_LOG=http_sse_demo=debug,axum=info cargo run
```

格式化与编译检查：

```bash
cargo fmt
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```

测试覆盖非法请求头、最大事件序号、中文提示词截断和聊天流完成事件。
