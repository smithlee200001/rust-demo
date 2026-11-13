//! 多进程通信（IPC）动态渲染进程管理 + 专职 IPC 线程 + 基准测试主程序
//!
//! 功能说明：
//! 1. 默认启动交互式多进程浏览器控制台（`multiplex_demo::run_interactive_browser`）：
//!    - 支持在 Terminal 中通过命令（`spawn`、`load`、`list`、`kill`、`quit`）动态创建并管理多个 Renderer 子进程。
//!    - 内置独立的专职 IPC 线程（`ipc-io-thread`），通过 `IpcReceiverSet` 多路复用所有子进程通道，绝不阻塞主线程。
//!    - 仅当用户在 Terminal 输入 `quit` 时才优雅关闭所有子进程并退出程序。
//! 2. 传入 `--bench` 参数时可运行三种 Protobuf + IPC 方案的吞吐量基准测试。

mod multiplex_demo;
mod scheme1_bytes;
mod scheme2_envelope;
mod scheme3_serde_proto;

use std::{collections::HashMap, env, time::Duration};

// 引入由 build.rs 编译 proto/ipc_messages.proto 生成的 Rust 代码
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/ipc_bench.rs"));
}

use proto::BenchPayload;

/// 基准测试统计结果结构体
pub struct BenchStats {
    /// 测试方案名称
    pub name: &'static str,
    /// 循环发送的消息总次数
    pub iterations: usize,
    /// 单条消息编码后的字节长度
    pub payload_bytes: usize,
    /// 跨进程发送+接收解码的总耗时
    pub elapsed: Duration,
}

impl BenchStats {
    /// 构造新的基准测试统计对象
    pub fn new(
        name: &'static str,
        iterations: usize,
        payload_bytes: usize,
        elapsed: Duration,
    ) -> Self {
        Self {
            name,
            iterations,
            payload_bytes,
            elapsed,
        }
    }

    /// 计算平均单条消息耗时（单位：微秒 µs）
    pub fn avg_latency_us(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1_000_000.0 / (self.iterations as f64)
    }

    /// 计算有效数据吞吐量（单位：MB/s）
    pub fn throughput_mb_s(&self) -> f64 {
        let total_mb = (self.iterations * self.payload_bytes) as f64 / (1024.0 * 1024.0);
        total_mb / self.elapsed.as_secs_f64()
    }
}

/// 构造指定 Body 字节长度的测试用 Protobuf 消息负载
fn build_sample_payload(body_size: usize) -> BenchPayload {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".into(), "RustChromiumDemo/1.0".into());
    headers.insert("Accept-Encoding".into(), "gzip, deflate, br".into());

    BenchPayload {
        seq_id: 0,
        tab_id: 42,
        url: "https://www.rust-lang.org/learn/get-started".into(),
        title: "Learn Rust - Rust Programming Language".into(),
        headers,
        body_data: vec![0xAB; body_size],
    }
}

/// 打印格式化的基准测试对比表（英文日志输出）
fn print_table(title: &str, results: &[BenchStats]) {
    println!(
        "\n=========================================================================================="
    );
    println!("  {title}");
    println!(
        "=========================================================================================="
    );
    println!(
        "{:<38} | {:>8} | {:>10} | {:>12} | {:>14}",
        "Scheme Name", "Messages", "Total(ms)", "Avg Lat(µs)", "Throughput(MB/s)"
    );
    println!(
        "------------------------------------------------------------------------------------------"
    );
    for r in results {
        println!(
            "{:<38} | {:>8} | {:>10.2} | {:>12.2} | {:>14.2}",
            r.name,
            r.iterations,
            r.elapsed.as_secs_f64() * 1000.0,
            r.avg_latency_us(),
            r.throughput_mb_s()
        );
    }
    println!(
        "=========================================================================================="
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // 1. 子进程模式路由：当主进程通过 Command::new 拉起当前可执行文件时，根据命令行参数进入对应子进程逻辑
    if args.len() >= 3 {
        match args[1].as_str() {
            "--mux-renderer" => {
                multiplex_demo::run_mux_renderer_worker(&args[2]);
                return;
            }
            "--worker-scheme1" if args.len() >= 4 => {
                let iterations: usize = args[3].parse().expect("Invalid iterations");
                scheme1_bytes::run_child(&args[2], iterations);
                return;
            }
            "--worker-scheme2" if args.len() >= 4 => {
                let iterations: usize = args[3].parse().expect("Invalid iterations");
                scheme2_envelope::run_child(&args[2], iterations);
                return;
            }
            "--worker-scheme3" if args.len() >= 4 => {
                let iterations: usize = args[3].parse().expect("Invalid iterations");
                scheme3_serde_proto::run_child(&args[2], iterations);
                return;
            }
            _ => {}
        }
    }

    // 2. 如果携带 --bench 参数，则运行三种 Protobuf 方案的性能基准测试
    if args.iter().any(|arg| arg == "--bench") {
        let small_payload = build_sample_payload(256);
        let small_iters = 5_000;

        let small_results = vec![
            scheme1_bytes::run_parent(&small_payload, small_iters),
            scheme2_envelope::run_parent(&small_payload, small_iters, false),
            scheme2_envelope::run_parent(&small_payload, small_iters, true),
            scheme3_serde_proto::run_parent(&small_payload, small_iters),
        ];
        print_table(
            "Scenario 1: High-Frequency Small Payload (Body = 256B, 5,000 messages)",
            &small_results,
        );

        let large_payload = build_sample_payload(512 * 1024);
        let large_iters = 200;

        let large_results = vec![
            scheme1_bytes::run_parent(&large_payload, large_iters),
            scheme2_envelope::run_parent(&large_payload, large_iters, false),
            scheme2_envelope::run_parent(&large_payload, large_iters, true),
            scheme3_serde_proto::run_parent(&large_payload, large_iters),
        ];
        print_table(
            "Scenario 2: Large Bulk Payload (Body = 512KB, 200 messages, ~100MB total)",
            &large_results,
        );
        return;
    }

    // 3. 默认启动支持终端交互动态创建 Renderer 子进程的控制台（含独立 IPC 线程）
    println!(
        "[Browser Main] Starting Interactive Multi-Process Browser (Main PID: {})",
        std::process::id()
    );
    multiplex_demo::run_interactive_browser();
}
