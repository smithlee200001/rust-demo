//! 基于 Axum 的 HTTP/SSE (Server-Sent Events) 示例。
//!
//! 本示例展示了两种最常见的 SSE 应用场景：
//! 1. `/sse/metrics`：模拟服务器实时监控指标推送，支持 `Last-Event-ID` 序号延续与心跳保活（不补发历史数据）。
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
    /// 用户输入的提示词（可选，未提供时使用默认提示词，空字符串按原样回显）。
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

/// 启动本机 HTTP 服务；绑定或服务错误向调用方返回，Ctrl+C 时由操作系统结束进程并关闭连接。
#[tokio::main]
async fn main() -> std::io::Result<()> {
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
    // 绑定成功后再输出启动日志，避免端口冲突时误报服务已启动。
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|err| {
        std::io::Error::new(
            err.kind(),
            format!("Failed to bind TCP listener on {addr}: {err}"),
        )
    })?;
    info!("Starting HTTP/SSE server on http://{}", addr);
    info!(
        "Open http://{} in your browser or run: curl -N http://{}/sse/metrics",
        addr, addr
    );

    // SSE 指标流可能无限持续，保留 Ctrl+C 的进程退出行为，由操作系统关闭连接。
    axum::serve(listener, app)
        .await
        .map_err(|err| std::io::Error::other(format!("HTTP server failed: {err}")))
}

/// 实时监控指标 SSE 处理器（`GET /sse/metrics`）。
///
/// # 参数说明
/// - `headers`: 客户端请求头集合，用于提取浏览器自动重连时携带的 `Last-Event-ID`。
///
/// # 关键逻辑与边界条件
/// - 若请求头中存在合法的 `Last-Event-ID`（无符号整数），则从 `Last-Event-ID + 1` 继续推送；
///   若不存在或解析失败，则默认从序号 `1` 开始推送；已达到 u64 上限时结束流。
/// - 每隔 1 秒推送一次 `event: metric` 事件，每条消息带有唯一 `id` 和 `retry: 3000`（重连等待 3 秒）。
/// - 配置 `KeepAlive`：若流空闲超过 15 秒，自动发送 `: keep-alive` 注释行防止代理超时断开。
async fn sse_metrics_handler(
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 只延续序号，不恢复历史数据；checked_add 防止上限处反复产生重复 ID。
    let start_seq = headers
        .get("last-event-id")
        .and_then(|val| val.to_str().ok())
        .and_then(|s| match s.parse::<u64>() {
            Ok(id) => Some(id.checked_add(1)),
            Err(_) => {
                warn!("Received invalid Last-Event-ID header value: {}", s);
                None
            }
        })
        .unwrap_or(Some(1));

    info!(
        "Client connected to /sse/metrics (starting sequence ID: {:?})",
        start_seq
    );

    // 使用 unfold 构建异步无限流，每次迭代生成一条指标数据并等待 1 秒
    let stream = stream::unfold(start_seq, |next_seq| async move {
        // 无可用序号时立即结束流，避免溢出及重复事件 ID。
        let seq = next_seq?;
        tokio::time::sleep(Duration::from_secs(1)).await;

        // 计算伪随机的 CPU 与内存模拟指标（利用三角函数与取模避免引入额外随机数库）
        let cpu_usage = 25.0 + ((seq as f32 * 0.7).sin().abs() * 55.0);
        // 先取模再乘法，保持原计算结果，同时避免外部序号过大导致溢出。
        let memory_mb = 512 + (seq % 256 * 13) % 256;
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

        // 发送最大序号后结束流；不回绕，也不重复最大 ID。
        Some((Ok(event), seq.checked_add(1)))
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
                .unwrap_or_else(|err| {
                    warn!("Failed to serialize chat token: {err}");
                    Event::default().event("error").data("Serialization error")
                });
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

/// 从 HTTP 响应层验证事件格式和极端请求头，覆盖真实 SSE 序列化路径。
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, response::IntoResponse};

    /// 最大序号之前的一次重连只能收到最后一个事件，然后结束，且指标计算不溢出。
    #[tokio::test]
    async fn metrics_stop_after_maximum_id() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "last-event-id",
            (u64::MAX - 1).to_string().parse().expect("Valid header"),
        );
        let response = sse_metrics_handler(headers).await.into_response();
        let bytes = to_bytes(response.into_body(), 8192)
            .await
            .expect("Valid SSE response");
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8 SSE response");
        assert!(text.contains(&format!("id: {}", u64::MAX)));
        assert!(text.contains("event: metric"));
        assert!(text.contains("\"memory_mb\":755"));
        assert_eq!(text.matches("event: metric").count(), 1);
    }

    /// 已耗尽序号时立即结束流，不再重复推送相同 ID。
    #[tokio::test]
    async fn metrics_end_when_last_id_is_exhausted() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "last-event-id",
            u64::MAX.to_string().parse().expect("Valid header"),
        );
        let response = sse_metrics_handler(headers).await.into_response();
        let bytes = to_bytes(response.into_body(), 8192)
            .await
            .expect("Valid empty response");
        assert!(bytes.is_empty());
    }

    /// 非法请求头回退到序号 1，且仍输出客户端需要的事件名与重连间隔。
    #[tokio::test]
    async fn metrics_invalid_header_starts_at_one() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "invalid".parse().expect("Valid header"));
        let mut stream = sse_metrics_handler(headers)
            .await
            .into_response()
            .into_body()
            .into_data_stream();
        let bytes = futures_util::StreamExt::next(&mut stream)
            .await
            .expect("First event")
            .expect("Valid event");
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8 SSE response");
        assert!(text.contains("id: 1\n"));
        assert!(text.contains("retry: 3000"));
        assert!(text.contains("event: metric"));
    }

    /// 多字节提示词按字符截断，流中包含 token 事件与唯一的完成标记。
    #[tokio::test]
    async fn chat_limits_unicode_prompt_and_finishes() {
        let response = sse_chat_handler(Query(ChatQuery {
            prompt: Some("你".repeat(101)),
        }))
        .await
        .into_response();
        let bytes = to_bytes(response.into_body(), 16384)
            .await
            .expect("Valid chat response");
        let text = String::from_utf8(bytes.to_vec()).expect("UTF-8 SSE response");
        assert_eq!(text.matches('你').count(), 100);
        assert!(text.contains("event: token"));
        assert_eq!(text.matches("event: done").count(), 1);
        assert!(text.contains("data: [DONE]"));
    }
}
