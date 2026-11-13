//! 多进程通信（IPC）动态渲染进程管理 + 专职 IPC 线程 + 基准测试主程序
//!
//! 功能说明：
//! 1. 默认启动交互式多进程浏览器控制台（`multiplex_demo::run_interactive_browser`）：
//!    - 支持在 Terminal 中通过命令（`spawn`、`load`、`list`、`kill`、`quit`）动态创建并管理多个 Renderer 子进程。
//!    - 内置独立的专职 IPC 线程（`ipc-io-thread`），通过 `IpcReceiverSet` 多路复用所有子进程通道，将 IPC 接收移出主异步任务。
//!    - 输入 `quit`、标准输入结束或读取失败时关闭子进程并退出程序。
//! 2. 传入 `--bench` 参数时可运行三种 Protobuf + IPC 方案的吞吐量基准测试。

mod multiplex_demo;
mod process_support;
mod scheme1_bytes;
mod scheme2_envelope;
mod scheme3_serde_proto;

use process_support::DemoResult;
use std::{collections::HashMap, env, io, process::ExitCode, time::Duration};

/// 由构建脚本生成的 Protobuf 消息类型。
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
    /// 单条消息的 Protobuf 参考字节长度；方案三实际使用 bincode，非实际传输字节数。
    pub payload_bytes: usize,
    /// 跨进程发送+接收解码的总耗时
    pub elapsed: Duration,
}

impl BenchStats {
    /// 构造统计对象：`name` 为方案名称，`iterations` 为发送次数，
    /// `payload_bytes` 为参考消息大小，`elapsed` 为发送和接收解码的总耗时。
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

    /// 计算平均单条消息耗时（微秒）；零次迭代返回 0，避免除零。
    pub fn avg_latency_us(&self) -> f64 {
        if self.iterations == 0 {
            return 0.0;
        }
        self.elapsed.as_secs_f64() * 1_000_000.0 / (self.iterations as f64)
    }

    /// 按 Protobuf 参考大小计算吞吐量（MiB/s），非物理链路吞吐量。
    /// 耗时为零时返回 0；先转换为浮点数再相乘，避免 usize 整数乘法溢出。
    pub fn throughput_mb_s(&self) -> f64 {
        if self.elapsed.is_zero() {
            return 0.0;
        }
        let total_mb = self.iterations as f64 * self.payload_bytes as f64 / (1024.0 * 1024.0);
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
        "Scheme Name", "Messages", "Total(ms)", "Avg Lat(µs)", "Throughput(MiB/s)"
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

/// 程序入口统一输出英文错误并返回失败状态，避免对用户输入或 IPC 错误触发 panic。
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Demo failed: {err}");
            ExitCode::FAILURE
        }
    }
}

/// 解析子进程迭代次数；拒绝零、负数、非法文本及超出 usize 范围的输入。
fn parse_iterations(value: &str) -> DemoResult<usize> {
    let iterations = value.parse::<usize>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Iterations must be a positive integer",
        )
    })?;
    if iterations == 0 {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "Iterations must be positive").into(),
        );
    }
    Ok(iterations)
}

/// 根据命令行选择交互模式、基准测试或内部工作进程；参数与运行错误向入口传播。
fn run() -> DemoResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Some(mode) = args.first() {
        match mode.as_str() {
            "--mux-renderer" if args.len() == 2 => {
                return multiplex_demo::run_mux_renderer_worker(&args[1]);
            }
            "--worker-scheme1" | "--worker-scheme2" | "--worker-scheme3" if args.len() == 3 => {
                let iterations = parse_iterations(&args[2])?;
                return match mode.as_str() {
                    "--worker-scheme1" => scheme1_bytes::run_child(&args[1], iterations),
                    "--worker-scheme2" => scheme2_envelope::run_child(&args[1], iterations),
                    _ => scheme3_serde_proto::run_child(&args[1], iterations),
                };
            }
            "--bench" if args.len() == 1 => {}
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "Usage: rust-demo [--bench]; worker modes require a server token and valid arguments").into()),
        }
    }

    // 2. 如果携带 --bench 参数，则运行三种 Protobuf 方案的性能基准测试
    if args.iter().any(|arg| arg == "--bench") {
        let small_payload = build_sample_payload(256);
        let small_iters = 5_000;

        let small_results = vec![
            scheme1_bytes::run_parent(&small_payload, small_iters)?,
            scheme2_envelope::run_parent(&small_payload, small_iters, false)?,
            scheme2_envelope::run_parent(&small_payload, small_iters, true)?,
            scheme3_serde_proto::run_parent(&small_payload, small_iters)?,
        ];
        print_table(
            "Scenario 1: High-Frequency Small Payload (Body = 256B, 5,000 messages)",
            &small_results,
        );

        let large_payload = build_sample_payload(512 * 1024);
        let large_iters = 200;

        let large_results = vec![
            scheme1_bytes::run_parent(&large_payload, large_iters)?,
            scheme2_envelope::run_parent(&large_payload, large_iters, false)?,
            scheme2_envelope::run_parent(&large_payload, large_iters, true)?,
            scheme3_serde_proto::run_parent(&large_payload, large_iters)?,
        ];
        print_table(
            "Scenario 2: Large Bulk Payload (Body = 512KB, 200 messages, ~100MB total)",
            &large_results,
        );
        return Ok(());
    }

    // 3. 默认启动支持终端交互动态创建 Renderer 子进程的控制台（含独立 IPC 线程）
    println!(
        "[Browser Main] Starting Interactive Multi-Process Browser (Main PID: {})",
        std::process::id()
    );
    multiplex_demo::run_interactive_browser()
}

/// 验证统计边界及内部工作进程参数，防止溢出或无效输入导致 panic。
#[cfg(test)]
mod tests {
    use super::*;

    /// 大计数应在浮点域计算；零耗时与零迭代均应返回有限结果。
    #[test]
    fn statistics_handle_empty_and_large_inputs() {
        let empty = BenchStats::new("empty", 0, 0, Duration::ZERO);
        assert_eq!(empty.avg_latency_us(), 0.0);
        assert_eq!(empty.throughput_mb_s(), 0.0);
        let large = BenchStats::new("large", usize::MAX, usize::MAX, Duration::from_secs(1));
        assert!(large.throughput_mb_s().is_finite());
        assert!(large.throughput_mb_s() > 0.0);
    }

    /// 用户参数错误应返回错误而非崩溃；正常正整数可以解析。
    #[test]
    fn worker_iterations_reject_invalid_values() {
        for value in ["0", "-1", "abc", "", "184467440737095516160"] {
            assert!(parse_iterations(value).is_err());
        }
        assert_eq!(
            parse_iterations("12").expect("Valid count should parse"),
            12
        );
    }
}
