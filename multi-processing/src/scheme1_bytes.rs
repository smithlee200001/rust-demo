//! 方案一：`ipc::bytes_channel` + `prost::Message` 纯字节流通道
//!
//! 特点：
//! 1. 引导阶段通过一次性的 `IpcOneShotServer` 交换 `IpcBytesSender` 和回复用的 `IpcBytesSender`。
//! 2. 数据传输阶段：完全绕过 `bincode`，直接将 `prost` 编码出的 Protobuf 二进制字节流写入底层 OS 管道。
//! 3. 接收端直接从字节流 `BenchPayload::decode(&bytes[..])`。

use crate::BenchStats;
use crate::process_support::{BenchChild, DemoResult, accept_worker};
use crate::proto::{BenchPayload, BenchSummary};
use ipc_channel::ipc::{self, IpcBytesSender, IpcOneShotServer, IpcSender};
use prost::Message;
use std::{io, time::Instant};

/// 运行父进程基准测试并返回统计结果。
///
/// `payload` 是待发送消息，`iterations` 必须大于零；通信或编解码失败时返回错误，
/// 已创建的子进程由守卫终止并回收，正常结束时检查回执数量与退出状态。
pub fn run_parent(payload: &BenchPayload, iterations: usize) -> DemoResult<BenchStats> {
    let (server, server_name) =
        IpcOneShotServer::<Option<(IpcBytesSender, IpcSender<IpcBytesSender>)>>::new()?;

    let mut child = BenchChild::spawn("--worker-scheme1", &server_name, iterations)?;

    // 1. 完成握手（不计入 IPC 传输耗时）
    let (cmd_bytes_tx, reply_setup_tx) = accept_worker(server, &server_name)?;
    let (reply_bytes_tx, reply_bytes_rx) = ipc::bytes_channel()?;
    reply_setup_tx.send(reply_bytes_tx)?;

    // 等待子进程发来 1 字节 Ready 信号，确保子进程已进入循环
    if reply_bytes_rx.recv()? != [1] {
        return Err(io::Error::other("Expected Ready byte from worker").into());
    }

    // 2. 开始正式计时：每次迭代执行 Protobuf 编码 + 跨进程发送
    let mut encode_buf = Vec::with_capacity(payload.encoded_len());
    let start = Instant::now();

    for i in 0..iterations {
        encode_buf.clear();
        let mut msg = payload.clone();
        msg.seq_id = i as u64;
        msg.encode(&mut encode_buf)?;
        cmd_bytes_tx.send(&encode_buf)?;
    }

    // 3. 等待子进程全部接收并解码完毕后的 Summary 回执
    let summary_bytes = reply_bytes_rx.recv()?;
    let summary = BenchSummary::decode(&summary_bytes[..])?;
    let elapsed = start.elapsed();

    child.wait_success()?;
    if summary.messages_received != iterations as u64 {
        return Err(io::Error::other("Worker received an unexpected number of messages").into());
    }

    Ok(BenchStats::new(
        "Scheme 1: bytes_channel + prost",
        iterations,
        encode_buf.len(),
        elapsed,
    ))
}

/// 连接父进程并处理指定数量的消息。
///
/// `server_name` 为父进程提供的握手令牌，`iterations` 为预期接收次数。
/// 连接中断、协议不匹配或解码失败时返回错误，由入口记录英文日志并退出。
pub fn run_child(server_name: &str, iterations: usize) -> DemoResult<()> {
    let bootstrap_tx = IpcSender::connect(server_name.to_string())?;

    // 创建接收主进程 Protobuf 字节流的通道
    let (cmd_bytes_tx, cmd_bytes_rx) = ipc::bytes_channel()?;
    let (reply_setup_tx, reply_setup_rx) = ipc::channel::<IpcBytesSender>()?;
    bootstrap_tx.send(Some((cmd_bytes_tx, reply_setup_tx)))?;

    let reply_bytes_tx = reply_setup_rx.recv()?;
    // 通知主进程已就绪
    reply_bytes_tx.send(&[1u8])?;

    let mut received_count = 0u64;
    let mut total_bytes_verified = 0u64;

    for _ in 0..iterations {
        let raw_bytes = cmd_bytes_rx.recv()?;
        let decoded = BenchPayload::decode(&raw_bytes[..])?;
        received_count += 1;
        // 累计字节数可能受外部消息大小影响，溢出时返回错误而不是回绕。
        total_bytes_verified = total_bytes_verified
            .checked_add(decoded.body_data.len() as u64)
            .ok_or_else(|| io::Error::other("Verified byte count overflow"))?;
    }

    let summary = BenchSummary {
        worker_pid: std::process::id(),
        messages_received: received_count,
        total_bytes_verified,
    };
    reply_bytes_tx.send(&summary.encode_to_vec())?;
    Ok(())
}
