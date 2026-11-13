//! 基于 Axum 的 HTTP/SSE (Server-Sent Events) 完整示例。
//!
//! 本示例展示了两种最常见的 SSE 应用场景：
//! 1. `/sse/metrics`：模拟服务器实时监控指标推送，支持 `Last-Event-ID` 断线续传与心跳保活。
//! 2. `/sse/chat`：模拟 AI 大模型流式打字机输出，逐 Token 推送并在结束时发送完成事件。
//! 3. `/`：内置单页 HTML 测试面板，可在浏览器中直观查看两种 SSE 流式效果。

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use axum::{
    Router,
    extract::Query,
    http::HeaderMap,
    response::{
        Html,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt as _;
use tracing::{info, warn};

/// 系统监控指标数据结构，用于序列化为 JSON 格式并通过 SSE 推送给客户端。
#[derive(Debug, Serialize)]
struct MetricPayload {
    /// 事件序号（递增 ID），用于标识当前消息顺序及断线重连恢复。
    seq: u64,
    /// 模拟的 CPU 使用率百分比（范围 0.0 ~ 100.0）。
    cpu_usage: f32,
    /// 模拟的内存使用量（单位：MB）。
    memory_mb: u64,
    /// 服务端生成该指标时的 Unix 时间戳（秒）。
    timestamp: u64,
}

/// AI 流式对话接口的查询参数。
#[derive(Debug, Deserialize)]
struct ChatQuery {
    /// 用户输入的提示词（可选，若为空则使用默认提示词）。
    prompt: Option<String>,
}

/// AI 流式输出的单个 Token 数据负载。
#[derive(Debug, Serialize)]
struct ChatTokenPayload {
    /// 当前推送的文本片段（Token）。
    token: String,
    /// 当前 Token 的索引位置（从 0 开始）。
    index: usize,
}

#[tokio::main]
async fn main() {
    // 初始化日志订阅器，默认输出 INFO 级别及以上的英文日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "http_sse_demo=info,axum=info".into()),
        )
        .init();

    // 构建 Axum 路由表：包含前端演示页与两个 SSE 流端点
    let app = Router::new()
        .route("/", get(index_handler))
        .route("/sse/metrics", get(sse_metrics_handler))
        .route("/sse/chat", get(sse_chat_handler));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Starting HTTP/SSE server on http://{}", addr);
    info!(
        "Open http://{} in your browser or run: curl -N http://{}/sse/metrics",
        addr, addr
    );

    // 绑定 TCP 监听端口并启动 HTTP 服务
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind TCP listener on 127.0.0.1:3000");

    axum::serve(listener, app)
        .await
        .expect("HTTP server encountered an unexpected fatal error");
}

/// 实时监控指标 SSE 处理器（`GET /sse/metrics`）。
///
/// # 参数说明
/// - `headers`: 客户端请求头集合，用于提取浏览器自动重连时携带的 `Last-Event-ID`。
///
/// # 关键逻辑与边界条件
/// - 若请求头中存在合法的 `Last-Event-ID`（无符号整数），则从 `Last-Event-ID + 1` 继续推送；
///   若不存在或解析失败，则默认从序号 `1` 开始推送。
/// - 每隔 1 秒推送一次 `event: metric` 事件，每条消息带有唯一 `id` 和 `retry: 3000`（重连等待 3 秒）。
/// - 配置 `KeepAlive`：若流空闲超过 15 秒，自动发送 `: keep-alive` 注释行防止代理超时断开。
async fn sse_metrics_handler(
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 解析 Last-Event-ID 请求头，支持断线重连后从上次断开的序号继续发送
    let start_seq = headers
        .get("last-event-id")
        .and_then(|val| val.to_str().ok())
        .and_then(|s| match s.parse::<u64>() {
            Ok(id) => Some(id.saturating_add(1)),
            Err(_) => {
                warn!("Received invalid Last-Event-ID header value: {}", s);
                None
            }
        })
        .unwrap_or(1);

    info!(
        "Client connected to /sse/metrics (starting sequence ID: {})",
        start_seq
    );

    // 使用 unfold 构建异步无限流，每次迭代生成一条指标数据并等待 1 秒
    let stream = stream::unfold(start_seq, |seq| async move {
        tokio::time::sleep(Duration::from_secs(1)).await;

        // 计算伪随机的 CPU 与内存模拟指标（利用三角函数与取模避免引入额外随机数库）
        let cpu_usage = 25.0 + ((seq as f32 * 0.7).sin().abs() * 55.0);
        let memory_mb = 512 + (seq * 13) % 256;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let payload = MetricPayload {
            seq,
            cpu_usage: (cpu_usage * 10.0).round() / 10.0,
            memory_mb,
            timestamp,
        };

        // 构造标准 SSE Event：指定 id、event 名称、重连间隔与 JSON 格式的 data
        let event = Event::default()
            .id(seq.to_string())
            .event("metric")
            .retry(Duration::from_millis(3000))
            .json_data(payload)
            .unwrap_or_else(|err| {
                warn!("Failed to serialize metric payload to JSON: {}", err);
                Event::default().event("error").data("Serialization error")
            });

        // unfold 返回当前产出的元素以及下一次迭代的状态（seq + 1，使用 saturating_add 防止溢出）
        Some((Ok(event), seq.saturating_add(1)))
    });

    // 包装为 Sse 响应并开启心跳保活机制
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// 模拟 AI 大模型流式打字机响应的 SSE 处理器（`GET /sse/chat`）。
///
/// # 参数说明
/// - `Query(query)`: URL 查询参数，包含可选的 `prompt` 字段。
///
/// # 关键逻辑与边界条件
/// - 将预设的回复拆分为多个词元（Token），每隔 150 毫秒推送一个 `event: token` 事件。
/// - 对输入 `prompt` 做长度截断边界保护（最多保留前 100 个字符），防止超长回显。
/// - 当所有词元推送完毕后，额外发送一条 `event: done` 事件通知客户端流已正常结束。
async fn sse_chat_handler(
    Query(query): Query<ChatQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let raw_prompt = query
        .prompt
        .unwrap_or_else(|| "What is HTTP/SSE?".to_string());

    // 边界保护：限制回显提示词的最大字符数为 100，避免恶意超长输入
    let safe_prompt: String = raw_prompt.chars().take(100).collect();
    info!(
        "Client started streaming chat response for prompt: {:?}",
        safe_prompt
    );

    // 构造模拟的分词片段列表
    let tokens: Vec<String> = vec![
        format!("You asked: \"{}\".\n\n", safe_prompt),
        "Server-Sent ".to_string(),
        "Events ".to_string(),
        "(SSE) ".to_string(),
        "allows ".to_string(),
        "a server ".to_string(),
        "to push ".to_string(),
        "real-time ".to_string(),
        "updates ".to_string(),
        "to the client ".to_string(),
        "over a single, ".to_string(),
        "long-lived ".to_string(),
        "HTTP connection!".to_string(),
    ];

    // 将每个分词转换为 `event: token` 事件
    let token_events: Vec<Result<Event, Infallible>> = tokens
        .into_iter()
        .enumerate()
        .map(|(index, token)| {
            let payload = ChatTokenPayload { token, index };
            let event = Event::default()
                .id(index.to_string())
                .event("token")
                .json_data(payload)
                .unwrap_or_else(|_| Event::default().data("{}"));
            Ok(event)
        })
        // 末尾追加一条 `event: done` 结束标志事件，方便前端关闭连接
        .chain(std::iter::once(Ok(Event::default()
            .event("done")
            .data("[DONE]"))))
        .collect();

    // 使用 throttle 控制每个事件的发送间隔为 150ms，模拟真实的流式生成速度
    let stream = stream::iter(token_events).throttle(Duration::from_millis(150));

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// 内置前端交互演示页处理器（`GET /`）。
///
/// 返回包含原生 `EventSource` 调用代码的单页 HTML，方便直接在浏览器中验证 SSE。
async fn index_handler() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <title>Rust Axum HTTP/SSE 演示</title>
    <style>
        body { font-family: system-ui, -apple-system, sans-serif; max-width: 820px; margin: 2rem auto; padding: 0 1rem; line-height: 1.6; color: #1f2937; }
        .card { border: 1px solid #e5e7eb; border-radius: 8px; padding: 1.25rem; margin-bottom: 1.5rem; box-shadow: 0 1px 2px rgba(0,0,0,0.05); }
        h1, h2 { margin-top: 0; }
        button { background: #2563eb; color: #fff; border: none; padding: 0.5rem 1rem; border-radius: 6px; cursor: pointer; margin-right: 0.5rem; }
        button.stop { background: #dc2626; }
        pre { background: #111827; color: #10b981; padding: 1rem; border-radius: 6px; min-height: 100px; max-height: 240px; overflow-y: auto; white-space: pre-wrap; }
        input { padding: 0.5rem; width: 300px; border: 1px solid #d1d5db; border-radius: 6px; margin-right: 0.5rem; }
    </style>
</head>
<body>
    <h1>Rust (Axum) HTTP/SSE 实时推送演示</h1>

    <div class="card">
        <h2>1. AI 流式打字机输出 (<code>/sse/chat</code>)</h2>
        <div>
            <input id="promptInput" type="text" value="What is HTTP/SSE?" />
            <button onclick="startChat()">发送并流式接收</button>
        </div>
        <pre id="chatOutput">点击上方按钮开始流式生成...</pre>
    </div>

    <div class="card">
        <h2>2. 实时系统指标监控 (<code>/sse/metrics</code>)</h2>
        <div>
            <button onclick="startMetrics()">建立 SSE 连接</button>
            <button class="stop" onclick="stopMetrics()">断开连接</button>
        </div>
        <pre id="metricsOutput">未连接</pre>
    </div>

    <script>
        let metricsSource = null;
        let chatSource = null;

        function startChat() {
            if (chatSource) chatSource.close();
            const prompt = encodeURIComponent(document.getElementById('promptInput').value);
            const output = document.getElementById('chatOutput');
            output.textContent = '';

            chatSource = new EventSource(`/sse/chat?prompt=${prompt}`);
            chatSource.addEventListener('token', (e) => {
                const data = JSON.parse(e.data);
                output.textContent += data.token;
            });
            chatSource.addEventListener('done', () => {
                output.textContent += '\n\n[流式传输完成，连接已关闭]';
                chatSource.close();
            });
        }

        function startMetrics() {
            if (metricsSource) metricsSource.close();
            const output = document.getElementById('metricsOutput');
            output.textContent = '正在连接 /sse/metrics ...\n';

            metricsSource = new EventSource('/sse/metrics');
            metricsSource.addEventListener('metric', (e) => {
                const data = JSON.parse(e.data);
                output.textContent = `[Event ID: ${e.lastEventId}] CPU: ${data.cpu_usage}% | Mem: ${data.memory_mb} MB | Time: ${data.timestamp}\n` + output.textContent;
            });
        }

        function stopMetrics() {
            if (metricsSource) {
                metricsSource.close();
                metricsSource = null;
                document.getElementById('metricsOutput').textContent = '[已手动断开连接]\n' + document.getElementById('metricsOutput').textContent;
            }
        }
    </script>
</body>
</html>"#,
    )
}
