//! 方案二：外层 `serde` 信封（管理句柄）+ 内层 Protobuf 负载（类 Chromium 架构）
//!
//! 包含两种子模式对比：
//! - 方案 2A (`Envelope + Vec<u8>`)：将 Protobuf 编码为 `Vec<u8>` 放入枚举变体中，外层经由 `bincode` 发送。
//! - 方案 2B (`Envelope + IpcSharedMemory`)：将 Protobuf 编码后放入零拷贝共享内存 `IpcSharedMemory`，
//!   跨进程仅传输虚拟内存页句柄，子进程直接从共享内存映射切片 `&shm[..]` 原地解码 Protobuf。

use crate::BenchStats;
use crate::proto::{BenchPayload, BenchSummary};
use ipc_channel::ipc::{self, IpcOneShotServer, IpcSender, IpcSharedMemory};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{env, process::Command, time::Instant};

/// 外层 IPC 控制信封（支持携带通道句柄、内联 Protobuf 字节流、或零拷贝共享内存）
#[derive(Serialize, Deserialize, Debug)]
pub enum EnvelopeToChild {
    /// 握手初始化：直接传递回复通道句柄
    Init {
        reply_tx: IpcSender<EnvelopeToParent>,
    },
    /// 方案 2A：内联 Protobuf 字节流（适合中小消息，但比方案一多一次 bincode 拷贝）
    InlineProtobuf(Vec<u8>),
    /// 方案 2B：零拷贝共享内存承载 Protobuf（适合大消息，内核态 0 拷贝）
    SharedMemoryProtobuf(IpcSharedMemory),
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum EnvelopeToParent {
    Ready,
    SummaryProtobuf(Vec<u8>),
}

pub fn run_parent(payload: &BenchPayload, iterations: usize, use_shm: bool) -> BenchStats {
    let (server, server_name) = IpcOneShotServer::<IpcSender<EnvelopeToChild>>::new().unwrap();

    let mut child = Command::new(env::current_exe().unwrap())
        .arg("--worker-scheme2")
        .arg(&server_name)
        .arg(iterations.to_string())
        .spawn()
        .expect("Failed to spawn scheme2 child process");

    // 1. 完成握手（不计入 IPC 传输耗时）
    let (_, cmd_tx) = server.accept().unwrap();
    let (reply_tx, reply_rx) = ipc::channel::<EnvelopeToParent>().unwrap();
    cmd_tx.send(EnvelopeToChild::Init { reply_tx }).unwrap();

    match reply_rx.recv().unwrap() {
        EnvelopeToParent::Ready => {}
        _ => panic!("Expected Ready message from child"),
    }

    // 2. 开始正式计时
    let mut encode_buf = Vec::with_capacity(payload.encoded_len());
    let start = Instant::now();

    for i in 0..iterations {
        encode_buf.clear();
        let mut msg = payload.clone();
        msg.seq_id = i as u64;
        msg.encode(&mut encode_buf).unwrap();

        if use_shm {
            // 方案 2B：放入共享内存传递句柄
            let shm = IpcSharedMemory::from_bytes(&encode_buf);
            cmd_tx
                .send(EnvelopeToChild::SharedMemoryProtobuf(shm))
                .unwrap();
        } else {
            // 方案 2A：直接传 Vec<u8>
            cmd_tx
                .send(EnvelopeToChild::InlineProtobuf(encode_buf.clone()))
                .unwrap();
        }
    }

    // 3. 等待子进程回执
    let summary = match reply_rx.recv().unwrap() {
        EnvelopeToParent::SummaryProtobuf(bytes) => BenchSummary::decode(&bytes[..]).unwrap(),
        _ => panic!("Expected SummaryProtobuf from child"),
    };
    let elapsed = start.elapsed();

    let _ = cmd_tx.send(EnvelopeToChild::Shutdown);
    let _ = child.wait();
    assert_eq!(summary.messages_received as usize, iterations);

    let name = if use_shm {
        "Scheme 2(B): Envelope + IpcSharedMemory"
    } else {
        "Scheme 2(A): Envelope + Inline Vec<u8>"
    };

    BenchStats::new(name, iterations, encode_buf.len(), elapsed)
}

pub fn run_child(server_name: &str, iterations: usize) {
    let bootstrap_tx = IpcSender::connect(server_name.to_string()).unwrap();
    let (cmd_tx, cmd_rx) = ipc::channel::<EnvelopeToChild>().unwrap();
    bootstrap_tx.send(cmd_tx).unwrap();

    let reply_tx = match cmd_rx.recv().unwrap() {
        EnvelopeToChild::Init { reply_tx } => reply_tx,
        _ => panic!("Expected Init message first"),
    };
    reply_tx.send(EnvelopeToParent::Ready).unwrap();

    let mut received_count = 0u64;
    let mut total_bytes_verified = 0u64;

    for _ in 0..iterations {
        match cmd_rx.recv().unwrap() {
            EnvelopeToChild::InlineProtobuf(bytes) => {
                let decoded = BenchPayload::decode(&bytes[..]).unwrap();
                received_count += 1;
                total_bytes_verified += decoded.body_data.len() as u64;
            }
            EnvelopeToChild::SharedMemoryProtobuf(shm) => {
                // 直接在映射的共享内存切片上零拷贝解码 Protobuf！
                let decoded = BenchPayload::decode(&shm[..]).unwrap();
                received_count += 1;
                total_bytes_verified += decoded.body_data.len() as u64;
            }
            EnvelopeToChild::Shutdown => break,
            _ => {}
        }
    }

    let summary = BenchSummary {
        worker_pid: std::process::id(),
        messages_received: received_count,
        total_bytes_verified,
    };
    reply_tx
        .send(EnvelopeToParent::SummaryProtobuf(summary.encode_to_vec()))
        .unwrap();
}
