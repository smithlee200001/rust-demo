//! 方案三：`.proto` 生成结构体派生 `serde::Serialize / Deserialize` 直接发送
//!
//! 特点：
//! 1. 在 `build.rs` 中通过 `config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")`
//!    让生成的 `BenchPayload` 直接支持 `serde`。
//! 2. 直接使用 `IpcSender<BenchPayload>` 发送结构体，无需手动调用 `.encode()` / `.decode()`。
//! 3. 注意底层机制：此时 `ipc-channel` 内部使用的是 `bincode` 对该结构体进行序列化，
//!    并没有走 Protobuf 的 Varint Wire Format 编码。

use crate::BenchStats;
use crate::proto::{BenchPayload, BenchSummary};
use ipc_channel::ipc::{self, IpcOneShotServer, IpcSender};
use prost::Message;
use std::{env, process::Command, time::Instant};

pub fn run_parent(payload: &BenchPayload, iterations: usize) -> BenchStats {
    let (server, server_name) =
        IpcOneShotServer::<(IpcSender<BenchPayload>, IpcSender<IpcSender<BenchSummary>>)>::new()
            .unwrap();

    let mut child = Command::new(env::current_exe().unwrap())
        .arg("--worker-scheme3")
        .arg(&server_name)
        .arg(iterations.to_string())
        .spawn()
        .expect("Failed to spawn scheme3 child process");

    // 1. 完成握手（不计入 IPC 传输耗时）
    let (_, (cmd_tx, reply_setup_tx)) = server.accept().unwrap();
    let (reply_tx, reply_rx) = ipc::channel::<BenchSummary>().unwrap();
    reply_setup_tx.send(reply_tx).unwrap();

    // 等待子进程就绪回执
    let _ = reply_rx.recv().unwrap();

    // 计算一下标准 Protobuf 大小供参考对比
    let wire_size = payload.encoded_len();

    // 2. 开始正式计时：直接将结构体交给 IpcSender<BenchPayload>（底层走 bincode）
    let start = Instant::now();

    for i in 0..iterations {
        let mut msg = payload.clone();
        msg.seq_id = i as u64;
        cmd_tx.send(msg).unwrap();
    }

    // 3. 等待子进程回执
    let summary = reply_rx.recv().unwrap();
    let elapsed = start.elapsed();

    let _ = child.wait();
    assert_eq!(summary.messages_received as usize, iterations);

    BenchStats::new(
        "Scheme 3: .proto + serde(bincode)",
        iterations,
        wire_size,
        elapsed,
    )
}

pub fn run_child(server_name: &str, iterations: usize) {
    let bootstrap_tx = IpcSender::connect(server_name.to_string()).unwrap();

    let (cmd_tx, cmd_rx) = ipc::channel::<BenchPayload>().unwrap();
    let (reply_setup_tx, reply_setup_rx) = ipc::channel::<IpcSender<BenchSummary>>().unwrap();
    bootstrap_tx.send((cmd_tx, reply_setup_tx)).unwrap();

    let reply_tx = reply_setup_rx.recv().unwrap();
    // 发送 Ready 信号
    reply_tx
        .send(BenchSummary {
            worker_pid: std::process::id(),
            messages_received: 0,
            total_bytes_verified: 0,
        })
        .unwrap();

    let mut received_count = 0u64;
    let mut total_bytes_verified = 0u64;

    for _ in 0..iterations {
        let decoded: BenchPayload = cmd_rx.recv().unwrap();
        received_count += 1;
        total_bytes_verified += decoded.body_data.len() as u64;
    }

    reply_tx
        .send(BenchSummary {
            worker_pid: std::process::id(),
            messages_received: received_count,
            total_bytes_verified,
        })
        .unwrap();
}
