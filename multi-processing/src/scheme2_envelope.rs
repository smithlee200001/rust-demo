//! 方案二：外层 `serde` 信封（管理句柄）+ 内层 Protobuf 负载（类 Chromium 架构）
//!
//! 包含两种子模式对比：
//! - 方案 2A (`Envelope + Vec<u8>`)：将 Protobuf 编码为 `Vec<u8>` 放入枚举变体中，外层经由 `bincode` 发送。
//! - 方案 2B (`Envelope + IpcSharedMemory`)：将 Protobuf 编码后复制到共享内存 `IpcSharedMemory`，
//!   跨进程传递共享内存句柄，子进程从映射切片解码；解码为拥有所有权的字段仍会分配与复制。

use crate::BenchStats;
use crate::process_support::{BenchChild, DemoResult, accept_worker};
use crate::proto::{BenchPayload, BenchSummary};
use ipc_channel::ipc::{self, IpcOneShotServer, IpcSender, IpcSharedMemory};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{io, time::Instant};

/// 外层 IPC 控制信封（支持携带通道句柄、内联 Protobuf 字节流、或共享内存）
#[derive(Serialize, Deserialize, Debug)]
pub enum EnvelopeToChild {
    /// 握手初始化：直接传递回复通道句柄
    Init {
        reply_tx: IpcSender<EnvelopeToParent>,
    },
    /// 方案 2A：内联 Protobuf 字节流（适合中小消息，但比方案一多一次 bincode 拷贝）
    InlineProtobuf(Vec<u8>),
    /// 方案 2B：共享内存承载 Protobuf（适合大消息，写入与解码仍有复制）
    SharedMemoryProtobuf(IpcSharedMemory),
    /// 提前结束接收循环；父进程会将不足预期数量的回执视为测试失败。
    Shutdown,
}

/// 子进程回复协议，区分就绪通知和完成统计。
#[derive(Serialize, Deserialize, Debug)]
pub enum EnvelopeToParent {
    /// 已建立回复通道，可以开始发送基准消息。
    Ready,
    /// Protobuf 编码的接收数量、子进程 PID 与累计消息体大小。
    SummaryProtobuf(Vec<u8>),
}

/// 运行父进程基准测试并返回统计结果。
///
/// `payload` 是待发送消息，`iterations` 必须大于零；通信或编解码失败时返回错误，
/// 已创建的子进程由守卫终止并回收，正常结束时检查回执数量与退出状态。
/// `use_shm` 为真时通过共享内存传输，否则使用内联字节数组。
pub fn run_parent(
    payload: &BenchPayload,
    iterations: usize,
    use_shm: bool,
) -> DemoResult<BenchStats> {
    let (server, server_name) = IpcOneShotServer::<Option<IpcSender<EnvelopeToChild>>>::new()?;

    let mut child = BenchChild::spawn("--worker-scheme2", &server_name, iterations)?;

    // 1. 完成握手（不计入 IPC 传输耗时）
    let cmd_tx = accept_worker(server, &server_name)?;
    let (reply_tx, reply_rx) = ipc::channel::<EnvelopeToParent>()?;
    cmd_tx.send(EnvelopeToChild::Init { reply_tx })?;

    match reply_rx.recv()? {
        EnvelopeToParent::Ready => {}
        _ => return Err(io::Error::other("Expected Ready message from child").into()),
    }

    // 2. 开始正式计时
    let mut encode_buf = Vec::with_capacity(payload.encoded_len());
    let start = Instant::now();

    for i in 0..iterations {
        encode_buf.clear();
        let mut msg = payload.clone();
        msg.seq_id = i as u64;
        msg.encode(&mut encode_buf)?;

        if use_shm {
            // 方案 2B：放入共享内存传递句柄
            let shm = IpcSharedMemory::from_bytes(&encode_buf);
            cmd_tx.send(EnvelopeToChild::SharedMemoryProtobuf(shm))?;
        } else {
            // 方案 2A：直接传 Vec<u8>
            cmd_tx.send(EnvelopeToChild::InlineProtobuf(encode_buf.clone()))?;
        }
    }

    // 3. 等待子进程回执
    let summary = match reply_rx.recv()? {
        EnvelopeToParent::SummaryProtobuf(bytes) => BenchSummary::decode(&bytes[..])?,
        _ => return Err(io::Error::other("Expected SummaryProtobuf from child").into()),
    };
    let elapsed = start.elapsed();

    child.wait_success()?;
    if summary.messages_received != iterations as u64 {
        return Err(io::Error::other("Worker received an unexpected number of messages").into());
    }

    let name = if use_shm {
        "Scheme 2(B): Envelope + IpcSharedMemory"
    } else {
        "Scheme 2(A): Envelope + Inline Vec<u8>"
    };

    Ok(BenchStats::new(name, iterations, encode_buf.len(), elapsed))
}

/// 连接父进程并处理指定数量的消息。
///
/// `server_name` 为父进程提供的握手令牌，`iterations` 为预期接收次数。
/// 连接中断、协议不匹配或解码失败时返回错误，由入口记录英文日志并退出。
pub fn run_child(server_name: &str, iterations: usize) -> DemoResult<()> {
    let bootstrap_tx = IpcSender::connect(server_name.to_string())?;
    let (cmd_tx, cmd_rx) = ipc::channel::<EnvelopeToChild>()?;
    bootstrap_tx.send(Some(cmd_tx))?;

    let reply_tx = match cmd_rx.recv()? {
        EnvelopeToChild::Init { reply_tx } => reply_tx,
        _ => return Err(io::Error::other("Expected Init message first").into()),
    };
    reply_tx.send(EnvelopeToParent::Ready)?;

    let mut received_count = 0u64;
    let mut total_bytes_verified = 0u64;

    for _ in 0..iterations {
        match cmd_rx.recv()? {
            EnvelopeToChild::InlineProtobuf(bytes) => {
                let decoded = BenchPayload::decode(&bytes[..])?;
                received_count += 1;
                // 累计字节数可能受外部消息大小影响，溢出时返回错误而不是回绕。
                total_bytes_verified = total_bytes_verified
                    .checked_add(decoded.body_data.len() as u64)
                    .ok_or_else(|| io::Error::other("Verified byte count overflow"))?;
            }
            EnvelopeToChild::SharedMemoryProtobuf(shm) => {
                // 从映射切片解码，String 和 Vec 字段仍需分配并复制。
                let decoded = BenchPayload::decode(&shm[..])?;
                received_count += 1;
                // 累计字节数可能受外部消息大小影响，溢出时返回错误而不是回绕。
                total_bytes_verified = total_bytes_verified
                    .checked_add(decoded.body_data.len() as u64)
                    .ok_or_else(|| io::Error::other("Verified byte count overflow"))?;
            }
            EnvelopeToChild::Shutdown => break,
            EnvelopeToChild::Init { .. } => {
                return Err(io::Error::other("Unexpected repeated Init message").into());
            }
        }
    }

    let summary = BenchSummary {
        worker_pid: std::process::id(),
        messages_received: received_count,
        total_bytes_verified,
    };
    reply_tx.send(EnvelopeToParent::SummaryProtobuf(summary.encode_to_vec()))?;
    Ok(())
}
