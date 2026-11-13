//! 方案一：`ipc::bytes_channel` + `prost::Message` 纯字节流通道
//!
//! 特点：
//! 1. 引导阶段通过一次性的 `IpcOneShotServer` 交换 `IpcBytesSender` 和回复用的 `IpcBytesSender`。
//! 2. 数据传输阶段：完全绕过 `bincode`，直接将 `prost` 编码出的 Protobuf 二进制字节流写入底层 OS 管道。
//! 3. 接收端直接从字节流 `BenchPayload::decode(&bytes[..])`。

use crate::BenchStats;
use crate::proto::{BenchPayload, BenchSummary};
use ipc_channel::ipc::{self, IpcBytesSender, IpcOneShotServer, IpcSender};
use prost::Message;
use std::{env, process::Command, time::Instant};

pub fn run_parent(payload: &BenchPayload, iterations: usize) -> BenchStats {
    let (server, server_name) =
        IpcOneShotServer::<(IpcBytesSender, IpcSender<IpcBytesSender>)>::new().unwrap();

    let mut child = Command::new(env::current_exe().unwrap())
        .arg("--worker-scheme1")
        .arg(&server_name)
        .arg(iterations.to_string())
        .spawn()
        .expect("Failed to spawn scheme1 child process");

    // 1. 完成握手（不计入 IPC 传输耗时）
    let (_, (cmd_bytes_tx, reply_setup_tx)) = server.accept().unwrap();
    let (reply_bytes_tx, reply_bytes_rx) = ipc::bytes_channel().unwrap();
    reply_setup_tx.send(reply_bytes_tx).unwrap();

    // 等待子进程发来 1 字节 Ready 信号，确保子进程已进入循环
    let _ = reply_bytes_rx.recv().unwrap();

    // 2. 开始正式计时：每次迭代执行 Protobuf 编码 + 跨进程发送
    let mut encode_buf = Vec::with_capacity(payload.encoded_len());
    let start = Instant::now();

    for i in 0..iterations {
        encode_buf.clear();
        let mut msg = payload.clone();
        msg.seq_id = i as u64;
        msg.encode(&mut encode_buf).unwrap();
        cmd_bytes_tx.send(&encode_buf).unwrap();
    }

    // 3. 等待子进程全部接收并解码完毕后的 Summary 回执
    let summary_bytes = reply_bytes_rx.recv().unwrap();
    let summary = BenchSummary::decode(&summary_bytes[..]).unwrap();
    let elapsed = start.elapsed();

    let _ = child.wait();
    assert_eq!(summary.messages_received as usize, iterations);

    BenchStats::new(
        "Scheme 1: bytes_channel + prost",
        iterations,
        encode_buf.len(),
        elapsed,
    )
}

pub fn run_child(server_name: &str, iterations: usize) {
    let bootstrap_tx = IpcSender::connect(server_name.to_string()).unwrap();

    // 创建接收主进程 Protobuf 字节流的通道
    let (cmd_bytes_tx, cmd_bytes_rx) = ipc::bytes_channel().unwrap();
    let (reply_setup_tx, reply_setup_rx) = ipc::channel::<IpcBytesSender>().unwrap();
    bootstrap_tx.send((cmd_bytes_tx, reply_setup_tx)).unwrap();

    let reply_bytes_tx = reply_setup_rx.recv().unwrap();
    // 通知主进程已就绪
    reply_bytes_tx.send(&[1u8]).unwrap();

    let mut received_count = 0u64;
    let mut total_bytes_verified = 0u64;

    for _ in 0..iterations {
        let raw_bytes = cmd_bytes_rx.recv().unwrap();
        let decoded = BenchPayload::decode(&raw_bytes[..]).unwrap();
        received_count += 1;
        total_bytes_verified += decoded.body_data.len() as u64;
    }

    let summary = BenchSummary {
        worker_pid: std::process::id(),
        messages_received: received_count,
        total_bytes_verified,
    };
    reply_bytes_tx.send(&summary.encode_to_vec()).unwrap();
}
