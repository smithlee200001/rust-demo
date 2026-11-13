//! 方案三：`.proto` 生成结构体派生 `serde::Serialize / Deserialize` 直接发送
//!
//! 特点：
//! 1. 在 `build.rs` 中通过 `config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")`
//!    让生成的 `BenchPayload` 直接支持 `serde`。
//! 2. 直接使用 `IpcSender<BenchPayload>` 发送结构体，无需手动调用 `.encode()` / `.decode()`。
//! 3. 注意底层机制：此时 `ipc-channel` 内部使用的是 `bincode` 对该结构体进行序列化，
//!    并没有走 Protobuf 的 Varint Wire Format 编码。

use crate::BenchStats;
use crate::process_support::{BenchChild, DemoResult, accept_worker};
use crate::proto::{BenchPayload, BenchSummary};
use ipc_channel::ipc::{self, IpcOneShotServer, IpcSender};
use prost::Message;
use std::{io, time::Instant};

/// 运行父进程基准测试并返回统计结果。
///
/// `payload` 是待发送消息，`iterations` 必须大于零；通信或编解码失败时返回错误，
/// 已创建的子进程由守卫终止并回收，正常结束时检查回执数量与退出状态。
pub fn run_parent(payload: &BenchPayload, iterations: usize) -> DemoResult<BenchStats> {
    let (server, server_name) = IpcOneShotServer::<
        Option<(IpcSender<BenchPayload>, IpcSender<IpcSender<BenchSummary>>)>,
    >::new()?;

    let mut child = BenchChild::spawn("--worker-scheme3", &server_name, iterations)?;

    // 1. 完成握手（不计入 IPC 传输耗时）
    let (cmd_tx, reply_setup_tx) = accept_worker(server, &server_name)?;
    let (reply_tx, reply_rx) = ipc::channel::<BenchSummary>()?;
    reply_setup_tx.send(reply_tx)?;

    // 等待子进程就绪回执
    let ready = reply_rx.recv()?;
    if ready.messages_received != 0 || ready.total_bytes_verified != 0 {
        return Err(io::Error::other("Expected empty Ready summary from worker").into());
    }

    // 计算一下标准 Protobuf 大小供参考对比
    let wire_size = payload.encoded_len();

    // 2. 开始正式计时：直接将结构体交给 IpcSender<BenchPayload>（底层走 bincode）
    let start = Instant::now();

    for i in 0..iterations {
        let mut msg = payload.clone();
        msg.seq_id = i as u64;
        cmd_tx.send(msg)?;
    }

    // 3. 等待子进程回执
    let summary = reply_rx.recv()?;
    let elapsed = start.elapsed();

    child.wait_success()?;
    if summary.messages_received != iterations as u64 {
        return Err(io::Error::other("Worker received an unexpected number of messages").into());
    }

    Ok(BenchStats::new(
        "Scheme 3: .proto + serde(bincode)",
        iterations,
        wire_size,
        elapsed,
    ))
}

/// 连接父进程并处理指定数量的消息。
///
/// `server_name` 为父进程提供的握手令牌，`iterations` 为预期接收次数。
/// 连接中断、协议不匹配或解码失败时返回错误，由入口记录英文日志并退出。
pub fn run_child(server_name: &str, iterations: usize) -> DemoResult<()> {
    let bootstrap_tx = IpcSender::connect(server_name.to_string())?;

    let (cmd_tx, cmd_rx) = ipc::channel::<BenchPayload>()?;
    let (reply_setup_tx, reply_setup_rx) = ipc::channel::<IpcSender<BenchSummary>>()?;
    bootstrap_tx.send(Some((cmd_tx, reply_setup_tx)))?;

    let reply_tx = reply_setup_rx.recv()?;
    // 发送 Ready 信号
    reply_tx.send(BenchSummary {
        worker_pid: std::process::id(),
        messages_received: 0,
        total_bytes_verified: 0,
    })?;

    let mut received_count = 0u64;
    let mut total_bytes_verified = 0u64;

    for _ in 0..iterations {
        let decoded: BenchPayload = cmd_rx.recv()?;
        received_count += 1;
        // 累计字节数可能受外部消息大小影响，溢出时返回错误而不是回绕。
        total_bytes_verified = total_bytes_verified
            .checked_add(decoded.body_data.len() as u64)
            .ok_or_else(|| io::Error::other("Verified byte count overflow"))?;
    }

    reply_tx.send(BenchSummary {
        worker_pid: std::process::id(),
        messages_received: received_count,
        total_bytes_verified,
    })?;
    Ok(())
}
