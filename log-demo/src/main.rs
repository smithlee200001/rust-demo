//! Rust `log` (0.4) 完整用法示例程序
//!
//! 本示例涵盖 `log` 库的核心功能与常见工程用法：
//! 1. 五种标准级别宏：`trace!`, `debug!`, `info!`, `warn!`, `error!`
//! 2. 动态日志级别宏：`log!(level, ...)`
//! 3. 自定义日志目标（`target`）：覆盖默认的模块路径，用于审计、性能指标等分类
//! 4. 条件日志判断宏（`log_enabled!`）：避免在低级别日志未开启时执行高开销计算
//! 5. 结构化键值对日志（`kv` 特性）：支持基础类型、`Debug` (`:?`)、`Display` (`:%`) 等结构化字段
//! 6. 子模块层级日志：演示不同子模块的默认 `target` 行为
//! 7. 运行时动态调整 `LevelFilter`
//! 8. `warn!` 宏配合 `serde_json::Value` 的多种用法（`{}`、`{:#}`、`{:?}`、`kv` 的 `:%` 与 `:serde`）
//! 9. 子进程 `stdout` 与 `stderr` 接管：并发读取子进程输出流并统一路由到 `log` 系统

mod child_process;

use log::{
    Level, LevelFilter, Log, Metadata, Record, debug, error, info,
    kv::{self, Key, Value, VisitSource},
    log, log_enabled, trace, warn,
};
use log_demo::file_log::{FileLog, Mode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// 自定义结构化键值对收集器
///
/// 用途：在自定义 `Log` 实现中遍历每条日志附带的 `kv` 结构化字段，并格式化为可读字符串。
struct KvCollector {
    /// 存储格式化后的 `[key=value, ...]` 缓冲区
    buffer: String,
}

impl KvCollector {
    /// 创建一个新的键值对收集器实例
    fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }
}

impl<'kvs> VisitSource<'kvs> for KvCollector {
    /// 访问单个结构化键值对并将其追加到缓冲区中
    ///
    /// 参数含义：
    /// - `key`: 结构化日志的键名
    /// - `value`: 结构化日志的值（实现了 `log::kv::Value`）
    ///
    /// 边界条件：当不是第一个键值对时，自动在前面追加逗号与空格分隔符。
    fn visit_pair(&mut self, key: Key<'kvs>, value: Value<'kvs>) -> Result<(), kv::Error> {
        if !self.buffer.is_empty() {
            self.buffer.push_str(", ");
        }
        use std::fmt::Write as _;
        let _ = write!(self.buffer, "{}={}", key, value);
        Ok(())
    }
}

/// 自定义日志记录器（演示如何对接 `log::Log` trait 并完整输出结构化 `kv` 与源码位置）
struct CustomLogger;

impl Log for CustomLogger {
    /// 判断指定元数据的日志是否应当被记录
    ///
    /// 参数含义：
    /// - `metadata`: 包含日志级别（`Level`）和目标分类（`target`）的元数据
    ///
    /// 关键逻辑：检查日志级别是否小于等于当前全局设置的最大日志级别（`log::max_level()`）。
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    /// 核心日志输出逻辑
    ///
    /// 参数含义：
    /// - `record`: 单条日志记录的完整上下文，包含消息体、级别、`target`、文件名、行号及结构化 `kv`
    ///
    /// 边界条件：
    /// - 若 `self.enabled(record.metadata())` 为 `false`，则直接跳过不输出；
    /// - 若文件名或行号缺失（例如宏展开异常），则使用 `"unknown"` 和 `0` 作为安全回退值。
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        // 收集结构化键值对（如果有）
        let mut kv_collector = KvCollector::new();
        let _ = record.key_values().visit(&mut kv_collector);

        let file = record.file().unwrap_or("unknown");
        let line = record.line().unwrap_or(0);

        let mut text = format!(
            "[{:<5}] [target: {:<22}] ({}:{}) {}",
            record.level(),
            record.target(),
            file,
            line,
            record.args()
        );
        if !kv_collector.buffer.is_empty() {
            use std::fmt::Write as _;
            let _ = write!(text, " {{ {} }}", kv_collector.buffer);
        }
        emit_line(&text);
    }

    /// 刷新当前去向的缓冲区；加密文件仍需要在退出前显式完成写入。
    fn flush(&self) {
        match &mut *DESTINATION.lock().unwrap_or_else(|e| e.into_inner()) {
            Destination::Console => {
                let _ = io::stdout().flush();
            }
            Destination::File(output) if output.error.is_none() => {
                output.error = output.writer.flush().err();
            }
            _ => {}
        }
    }
}

/// 全局静态自定义 Logger 实例。
static CUSTOM_LOGGER: CustomLogger = CustomLogger;

/// 文件失败后保留首个错误，不把后续日志回退到控制台。
struct FileOutput {
    writer: FileLog,
    error: Option<io::Error>,
}

/// 父进程普通记录和子进程转发共用一条串行输出路径。
enum Destination {
    Console,
    File(Box<FileOutput>),
    Closed,
}

static DESTINATION: Mutex<Destination> = Mutex::new(Destination::Console);

/// 写入一条已格式化记录。保留子进程原有级别和格式，同时不能跳过脱敏和加密。
fn emit_line(line: &str) {
    match &mut *DESTINATION.lock().unwrap_or_else(|e| e.into_inner()) {
        Destination::Console => {
            let _ = writeln!(io::stdout().lock(), "{line}");
        }
        Destination::File(output) if output.error.is_none() => {
            output.error = output.writer.append(line).err();
        }
        _ => {}
    }
}

/// 所有生产者退出后关闭写入器，使错误能影响进程退出码。
fn finish_output() -> io::Result<()> {
    let destination = {
        let mut output = DESTINATION.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::replace(&mut *output, Destination::Closed)
    };
    if let Destination::File(output) = destination {
        if let Some(error) = output.error {
            // 部分写入过的加密流不尝试继续修补；显式返回失败。
            return Err(error);
        }
        output.writer.finish()?;
    }
    Ok(())
}

const USAGE: &str = "Usage: log-demo [--file PATH [--encryption plain|symmetric|asymmetric] [--recipient AGE_PUBLIC_KEY] [--no-redact]]";

/// 命令行加密选项只在边界解析为枚举，口令不出现在参数或环境变量中。
#[derive(Clone, Copy)]
enum Algorithm {
    Plain,
    Symmetric,
    Asymmetric,
}

enum CommandLine {
    Help,
    Child,
    Console,
    File {
        path: PathBuf,
        key: Mode,
        redact: bool,
    },
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// 校验参数，各项仅允许指定一次。不认识的选项不会静默降级。
fn parse_args() -> io::Result<CommandLine> {
    let mut args = std::env::args_os().skip(1).peekable();
    if args.peek().is_none() {
        return Ok(CommandLine::Console);
    }
    if args.peek().and_then(|s| s.to_str()) == Some("--help") {
        args.next();
        return if args.next().is_none() {
            Ok(CommandLine::Help)
        } else {
            Err(invalid(USAGE))
        };
    }
    if args.peek().and_then(|s| s.to_str()) == Some("--child-worker") {
        args.next();
        return if args.next().is_none() {
            Ok(CommandLine::Child)
        } else {
            Err(invalid(USAGE))
        };
    }
    let mut path = None;
    let mut algorithm = None;
    let mut public_key = None;
    let mut redact = true;
    while let Some(flag) = args.next() {
        match flag.to_str() {
            Some("--file") if path.is_none() => {
                let value = args.next().ok_or_else(|| invalid("Missing file path"))?;
                if value.is_empty() || value.to_str().is_some_and(|s| s.starts_with("--")) {
                    return Err(invalid("Missing file path"));
                }
                path = Some(PathBuf::from(value));
            }
            Some("--encryption") if algorithm.is_none() => {
                algorithm = Some(
                    match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
                        Some("plain") => Algorithm::Plain,
                        Some("symmetric") => Algorithm::Symmetric,
                        Some("asymmetric") => Algorithm::Asymmetric,
                        _ => return Err(invalid("Unknown or missing encryption option")),
                    },
                );
            }
            Some("--recipient") if public_key.is_none() => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid("Missing age public recipient"))?;
                public_key = Some(
                    value
                        .to_str()
                        .and_then(|s| s.parse::<age::x25519::Recipient>().ok())
                        .ok_or_else(|| invalid("Invalid age X25519 public recipient"))?,
                );
            }
            Some("--no-redact") if redact => redact = false,
            _ => return Err(invalid(USAGE)),
        }
    }
    let path = path.ok_or_else(|| invalid("File options require --file PATH"))?;
    let key = match (algorithm.unwrap_or(Algorithm::Plain), public_key) {
        (Algorithm::Plain, None) => Mode::Plain,
        (Algorithm::Symmetric, None) => {
            let secret = rpassword::prompt_password("Log encryption passphrase: ")?;
            let secret = age::secrecy::SecretString::from(secret);
            use age::secrecy::ExposeSecret as _;
            if secret.expose_secret().trim().is_empty() {
                return Err(invalid("Log encryption passphrase must not be blank"));
            }
            Mode::Symmetric(secret)
        }
        (Algorithm::Asymmetric, Some(recipient)) => Mode::Asymmetric(recipient),
        (Algorithm::Asymmetric, None) => {
            return Err(invalid("Asymmetric encryption requires --recipient"));
        }
        _ => {
            return Err(invalid(
                "--recipient is only valid for asymmetric encryption",
            ));
        }
    };
    Ok(CommandLine::File { path, key, redact })
}

/// 演示用于结构化打印的用户角色枚举（使用强类型替代裸字符串，并映射为 `snake_case`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum UserRole {
    /// 平台管理员
    PlatformAdmin,
    /// 普通成员
    Member,
    /// 访客
    Guest,
}

/// 演示用于结构化打印的用户结构体（派生 `Debug`）
#[derive(Debug)]
#[allow(dead_code)]
struct UserContext {
    /// 用户唯一 ID
    user_id: u64,
    /// 用户角色
    role: UserRole,
}

/// 演示用于结构化打印的业务错误类型（实现 `Display` 与 `Debug`）
#[derive(Debug)]
struct PaymentError {
    /// 错误码
    code: u32,
    /// 错误原因描述
    reason: &'static str,
}

impl fmt::Display for PaymentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PaymentError(code={}, reason={})",
            self.code, self.reason
        )
    }
}

/// 1. 演示五种基础日志级别宏：`trace!`, `debug!`, `info!`, `warn!`, `error!`
fn demo_basic_levels() {
    info!("=== 1. Demonstrating standard log levels ===");

    trace!("Entering demo_basic_levels function with detailed execution trace");
    debug!("Debugging internal state: connection_pool_size = {}", 16);
    info!("Application service initialized on port {}", 8080);
    warn!("High memory usage detected: {}% of limit used", 85);
    error!("Failed to synchronize cache with remote replica");
}

/// 2. 演示通用 `log!` 宏（支持在运行时动态决定日志级别）
///
/// 参数含义：
/// - `latency_ms`: 请求耗时（毫秒）
///
/// 关键逻辑与边界条件：
/// - 耗时 >= 1000ms 时使用 `Level::Error`
/// - 耗时 >= 200ms 时使用 `Level::Warn`
/// - 耗时 < 200ms 时使用 `Level::Info`
fn demo_dynamic_level(latency_ms: u64) {
    let level = if latency_ms >= 1000 {
        Level::Error
    } else if latency_ms >= 200 {
        Level::Warn
    } else {
        Level::Info
    };

    // 使用 `log!` 宏传入动态计算出的 `Level`
    log!(
        level,
        "HTTP request finished with dynamic severity evaluation (latency = {}ms)",
        latency_ms
    );
}

/// 3. 演示自定义 `target` 参数
///
/// 用途：默认情况下，`target` 是当前模块路径（如 `log_demo` 或 `log_demo::database`）。
/// 通过显式指定 `target: "..."`，可以将安全审计、SQL 执行、计费流水等日志按逻辑分类路由。
fn demo_custom_target() {
    info!("=== 3. Demonstrating custom target specification ===");

    info!(
        target: "security::audit",
        "Administrator updated system permission policy"
    );
    warn!(
        target: "network::rate_limiter",
        "Client IP 192.168.1.100 is approaching rate limit threshold"
    );
    error!(
        target: "billing::settlement",
        "Transaction reconciliation mismatch detected for batch #20261106"
    );
}

/// 4. 演示 `log_enabled!` 条件判断宏
///
/// 用途：当生成日志参数本身需要高昂开销（例如遍历大型集合、序列化大对象、复杂统计计算）时，
/// 先通过 `log_enabled!` 检查当前级别是否开启，避免无意义的 CPU/内存消耗。
fn demo_log_enabled_guard() {
    info!("=== 4. Demonstrating log_enabled! lazy evaluation guard ===");

    if log_enabled!(Level::Trace) {
        // 模拟高开销计算：仅当 Trace 级别开启时才会执行这段代码
        let expensive_dump: Vec<u32> = (1..=5).map(|x| x * 100).collect();
        trace!(
            "Expensive diagnostic dump computed because TRACE is active: {:?}",
            expensive_dump
        );
    } else {
        warn!("Skipped expensive TRACE diagnostic computation because TRACE level is disabled");
    }

    // 也可以同时结合自定义 target 判断特定分类的级别是否开启
    if log_enabled!(target: "metrics::profiler", Level::Debug) {
        debug!(
            target: "metrics::profiler",
            "CPU profiling snapshot captured: 4 active worker threads"
        );
    }
}

/// 5. 演示 `log` 0.4 的结构化键值对（Key-Value）用法
///
/// 关键语法（注意：在 `log` crate 中，键值对与消息字符串之间使用分号 `;` 分隔）：
/// - `key = value`: 记录实现了 `Value` 的基础类型（整数、浮点数、布尔、字符串等）
/// - `key:? = value`: 使用 `fmt::Debug` 格式化复杂结构体
/// - `key:% = value`: 使用 `fmt::Display` 格式化自定义类型或错误
/// - `target: "...", k1 = v1; "message"`: 同时组合自定义 `target` 与结构化字段
fn demo_structured_kv() {
    info!("=== 5. Demonstrating structured Key-Value (kv) logging ===");

    let order_id = 9527_u64;
    let amount_usd = 49.99_f64;
    let is_vip = true;

    // 5.1 基础类型键值对（也可以简写同名变量，如 `order_id` 等价于 `order_id = order_id`）
    info!(
        order_id,
        amount_usd,
        is_vip;
        "Order checkout completed successfully"
    );

    info!(path = "/home/demo_linux_user/report.txt"; "Read a synthetic report path");

    // 5.2 使用 `:?` 记录实现了 Debug 的结构体
    let current_user = UserContext {
        user_id: 10086,
        role: UserRole::PlatformAdmin,
    };
    debug!(
        user:? = current_user,
        session_ttl_sec = 3600;
        "Loaded authenticated user context into request scope"
    );

    // 5.3 使用 `:%` 记录实现了 Display 的错误对象，并结合自定义 target
    let payment_err = PaymentError {
        code: 5003,
        reason: "Insufficient account balance",
    };
    error!(
        target: "payment::gateway",
        order_id,
        err:% = payment_err,
        retryable = false;
        "Payment execution rejected by upstream bank"
    );
}

/// 6. 子模块日志演示：展示模块路径如何自动成为默认的 `target`
mod database {
    use log::{debug, info, warn};

    /// 模拟数据库连接与慢查询日志
    ///
    /// 参数含义：
    /// - `query_time_ms`: SQL 查询耗时（毫秒）
    pub fn execute_query(query_time_ms: u64) {
        // 此处未指定 target，默认 target 会自动设为 `log_demo::database`
        debug!(
            query = "SELECT id, name FROM users WHERE active = true",
            query_time_ms;
            "Executing SQL statement"
        );

        if query_time_ms > 100 {
            warn!(
                query_time_ms,
                threshold_ms = 100;
                "Slow database query detected"
            );
        } else {
            info!(query_time_ms; "Database query finished within SLA");
        }
    }
}

/// 7. 演示运行时动态修改全局 `LevelFilter` 的效果
///
/// 关键逻辑：通过 `log::set_max_level` 动态调整日志过滤阈值，对比 `Trace` 开启前后的行为差异。
fn demo_runtime_level_filter_switch() {
    info!("=== 7. Demonstrating runtime LevelFilter switching ===");

    // 将全局日志级别临时收紧到 Warn（此时 Info / Debug / Trace 都会被过滤）
    log::set_max_level(LevelFilter::Warn);
    info!("This INFO message will NOT be printed because max_level is Warn");
    warn!("Global log level is now restricted to WARN and ERROR only");
    demo_log_enabled_guard();

    // 重新将全局日志级别放开到 Trace（所有级别均可输出）
    log::set_max_level(LevelFilter::Trace);
    info!("Restored global log level back to TRACE");
    demo_log_enabled_guard();
}

/// 8. 演示 `warn!` 宏传入 `serde_json::Value` 的各种方式
///
/// 关键逻辑：
/// `serde_json::Value` 同时实现了 `Display`、`Debug` 和 `serde::Serialize`，
/// 因此在 `warn!` 中既可以作为格式化字符串参数，也可以作为结构化 `kv` 字段使用。
fn demo_serde_json_value() {
    info!("=== 8. Demonstrating warn! with serde_json::Value ===");

    // 构造一个包含嵌套字段的 serde_json::Value 对象
    let webhook_payload = json!({
        "event": "payment.retry",
        "attempt": 3,
        "meta": {
            "gateway": "stripe",
            "degraded": true
        }
    });

    // 方式 1：在格式化字符串中使用 `{}`（调用 Display，输出紧凑的单行 JSON 字符串，最推荐）
    warn!(
        "Webhook payload contains deprecated field (compact JSON): {}",
        webhook_payload
    );

    // 方式 2：在格式化字符串中使用 `{:#}`（调用 Alternate Display，输出带缩进的多行美化 JSON）
    warn!(
        "Webhook payload pretty-printed for inspection:\n{:#}",
        webhook_payload
    );

    // 方式 3：在格式化字符串中使用 `{:?}`（调用 Debug，输出 Value 枚举内部结构）
    warn!(
        "Webhook payload internal Debug representation: {:?}",
        webhook_payload
    );

    // 方式 4：在结构化 kv 中使用 `:%` 修饰符（无需开启 kv_serde，只要开启 kv 即可使用 Display）
    warn!(
        source = "webhook_handler",
        payload:% = webhook_payload;
        "Received degraded webhook notification (kv Display modifier)"
    );

    // 方式 5：在结构化 kv 中使用 `:serde` 修饰符（需开启 log 的 `kv_serde` 特性，保留结构化序列化能力）
    warn!(
        target: "webhook::audit",
        payload:serde = webhook_payload;
        "Recorded structured serde_json::Value in warning log (kv_serde modifier)"
    );

    warn!(
        target: "webhook::audit",
        payload:serde = webhook_payload;
        "Recorded structured serde_json::Value in warning log (kv_serde modifier)"
    );
}

/// 9. 演示接管子进程的 `stdout` 和 `stderr` 并统一接入 `log` 日志系统
///
/// 关键逻辑：
/// 通过 `std::env::current_exe()` 启动当前二进制的 `--child-worker` 子进程模式，
/// 使用 `child_process::run_and_log_child_process` 并发采集其标准输出与标准错误。
fn demo_child_process_capture() {
    info!("=== 9. Demonstrating child process stdout/stderr log capture ===");

    let exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            error!("Failed to resolve current executable path: {}", err);
            return;
        }
    };

    let mut command = std::process::Command::new(exe_path);
    command.arg("--child-worker");

    match child_process::run_and_log_child_process(&mut command, "demo_worker") {
        Ok(summary) => {
            info!(
                target: "child::summary",
                exit_code = summary.status.code().unwrap_or(-1),
                stdout_lines = summary.stdout_lines,
                stderr_lines = summary.stderr_lines;
                "Completed child process output capture"
            );
        }
        Err(err) => {
            error!(
                target: "child::summary",
                "Failed to run and capture child process output: {}",
                err
            );
        }
    }
}

fn main() -> io::Result<()> {
    let command = parse_args()?;
    let child = matches!(command, CommandLine::Child);
    match command {
        CommandLine::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        CommandLine::File { path, key, redact } => {
            let writer = FileLog::create(&path, key, redact)?;
            *DESTINATION.lock().unwrap_or_else(|e| e.into_inner()) =
                Destination::File(Box::new(FileOutput {
                    writer,
                    error: None,
                }));
        }
        _ => {}
    }

    // 子进程只写自己的管道；父进程统一处理文件。仅命令行选中文件时关闭控制台镜像。
    log::set_logger(&CUSTOM_LOGGER).map_err(io::Error::other)?;
    log::set_max_level(LevelFilter::Trace);
    if child {
        child_process::run_child_worker();
        log::logger().flush();
        return Ok(());
    }

    info!("Starting rust-lang/log 0.4 comprehensive demonstration");

    // 1. 标准五级日志
    demo_basic_levels();

    // 2. 动态日志级别 (`log!` 宏)
    info!("=== 2. Demonstrating dynamic level with log! macro ===");
    demo_dynamic_level(45);
    demo_dynamic_level(350);
    demo_dynamic_level(1500);

    // 3. 自定义 target
    demo_custom_target();

    // 4. 条件判断 `log_enabled!`
    demo_log_enabled_guard();

    // 5. 结构化键值对 (kv)
    demo_structured_kv();

    // 6. 子模块默认 target (`log_demo::database`)
    info!("=== 6. Demonstrating sub-module default target ===");
    database::execute_query(25);
    database::execute_query(240);

    // 7. 运行时动态调整日志过滤级别
    demo_runtime_level_filter_switch();

    // 8. 演示 warn! 宏配合 serde_json::Value 的各种用法
    demo_serde_json_value();

    // 9. 演示接管子进程的 stdout 和 stderr
    demo_child_process_capture();

    // 最后一条也必须在完成加密流之前写入；若落盘失败由进程返回非零退出码。
    info!("All log demonstrations completed successfully");
    log::logger().flush();
    finish_output()
}
