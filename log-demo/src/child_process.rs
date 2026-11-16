//! 子进程标准输出与标准错误接管模块
//!
//! 本模块演示如何通过 `std::process::Command` 启动子进程，并发接管其
//! `stdout` 与 `stderr` 管道，并将每一行输出按日志级别和结构化字段
//! 统一路由到 `log` 日志系统中。
//!
//! 关键设计点：
//! 1. 使用 `std::thread::scope` 并发读取 `stdout` 与 `stderr`，避免单个管道写满导致的父子进程死锁。
//! 2. 使用 `BufReader::read_until(b'\n', ...)` 配合 `String::from_utf8_lossy` 逐行读取，
//!    既能实时流式输出，也能在子进程输出非 UTF-8 字节时不中断后续日志采集。
//! 3. 无论管道读取是否出错，均会调用 `child.wait()` 回收子进程，防止产生僵尸进程。

use log::{error, info, warn};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

/// 子进程执行完成后的输出统计与退出状态摘要
#[derive(Debug)]
pub struct ChildOutputSummary {
    /// 子进程的退出状态
    pub status: ExitStatus,
    /// 从标准输出（`stdout`）采集到的总行数
    pub stdout_lines: usize,
    /// 从标准错误（`stderr`）采集到的总行数
    pub stderr_lines: usize,
}

/// 逐行读取字节流并回调处理每一行文本
///
/// 参数含义：
/// - `reader`: 任意实现了 `std::io::Read` 的输入流（如子进程 `stdout` / `stderr` 管道）
/// - `on_line`: 每读取到一行（已去除末尾 `\r` / `\n`）时触发的回调函数
///
/// 返回值：成功时返回读取到的总行数；遇到底层 I/O 错误时返回 `io::Error`。
///
/// 边界条件：
/// - 若末尾最后一行没有换行符但包含数据，到达 EOF 时仍会被完整读取并回调。
/// - 若流中包含非法 UTF-8 字节序列，将使用替代字符（U+FFFD）安全解码，不会提前终止读取。
fn pump_stream<R, F>(reader: R, mut on_line: F) -> io::Result<usize>
where
    R: Read,
    F: FnMut(&str),
{
    let mut buf_reader = BufReader::new(reader);
    let mut line_buf = Vec::new();
    let mut line_count = 0_usize;

    loop {
        line_buf.clear();
        let bytes_read = buf_reader.read_until(b'\n', &mut line_buf)?;
        if bytes_read == 0 {
            break;
        }

        // 统一移除行尾的 `\n` 与 `\r`，兼容 Unix（LF）与 Windows（CRLF）换行
        while line_buf.ends_with(b"\n") || line_buf.ends_with(b"\r") {
            line_buf.pop();
        }

        let line = String::from_utf8_lossy(&line_buf);
        on_line(line.as_ref());
        line_count += 1;
    }

    Ok(line_count)
}

/// 启动子进程并接管其 `stdout` 与 `stderr` 到 `log` 系统
///
/// 参数含义：
/// - `command`: 待执行的 `Command` 配置，本函数会自动将其 `stdout` 和 `stderr` 重定向为 `Stdio::piped()`
/// - `child_name`: 用于日志结构化字段 `child` 的逻辑名称，便于区分不同子任务
///
/// 返回值：成功时返回包含退出状态与采集行数的 [`ChildOutputSummary`]；
/// 若子进程启动失败、管道读取失败或等待子进程退出失败，则返回 `io::Error`。
///
/// 并发与资源约束：
/// - 在 `std::thread::scope` 中分别启动两个线程并发消费 `stdout` 和 `stderr`，
///   确保两者互不阻塞且在作用域结束前完成回收。
/// - 即使任一管道读取发生错误，仍会执行 `child.wait()` 以避免僵尸进程。
pub fn run_and_log_child_process(
    command: &mut Command,
    child_name: &str,
) -> io::Result<ChildOutputSummary> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn()?;
    let pid = child.id();

    info!(
        target: "child::lifecycle",
        child = child_name,
        pid;
        "Spawned child process with piped stdout and stderr"
    );

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Failed to capture child stdout pipe"))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Failed to capture child stderr pipe"))?;

    // 使用作用域线程并发读取 stdout 与 stderr，防止任一管道缓冲区写满引发死锁
    let (stdout_res, stderr_res) = thread::scope(|scope| {
        let stdout_handle = scope.spawn(|| {
            pump_stream(stdout_pipe, |line| {
                info!(
                    target: "child::stdout",
                    child = child_name,
                    pid;
                    "{}",
                    line
                );
            })
        });

        let stderr_handle = scope.spawn(|| {
            pump_stream(stderr_pipe, |line| {
                warn!(
                    target: "child::stderr",
                    child = child_name,
                    pid;
                    "{}",
                    line
                );
            })
        });

        let out = stdout_handle
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("Child stdout reader thread panicked")));
        let err = stderr_handle
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("Child stderr reader thread panicked")));

        (out, err)
    });

    // 无论管道读取是否出错，都必须等待子进程结束以释放系统进程表资源
    let status = child.wait()?;
    let stdout_lines = stdout_res?;
    let stderr_lines = stderr_res?;
    let exit_code = status.code().unwrap_or(-1);

    if status.success() {
        info!(
            target: "child::lifecycle",
            child = child_name,
            pid,
            exit_code,
            stdout_lines,
            stderr_lines;
            "Child process exited normally"
        );
    } else {
        error!(
            target: "child::lifecycle",
            child = child_name,
            pid,
            exit_code,
            stdout_lines,
            stderr_lines;
            "Child process exited with non-zero status"
        );
    }

    Ok(ChildOutputSummary {
        status,
        stdout_lines,
        stderr_lines,
    })
}

/// 模拟子进程工作模式（通过 `--child-worker` 参数触发）
///
/// 用途：让当前可执行文件自身充当子进程，交替向标准输出与标准错误写入多行英文消息，
/// 从而在不依赖外部脚本或系统命令的情况下自包含地演示子进程输出接管能力。
pub fn run_child_worker() {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();

    let _ = writeln!(stdout, "Worker initialized: loading configuration");
    let _ = stdout.flush();
    thread::sleep(Duration::from_millis(15));

    let _ = writeln!(
        stderr,
        "Deprecated environment variable detected, falling back to default"
    );
    let _ = stderr.flush();
    thread::sleep(Duration::from_millis(15));

    let _ = writeln!(stdout, "Processed batch #1 (128 records)");
    let _ = stdout.flush();
    thread::sleep(Duration::from_millis(15));

    let _ = writeln!(
        stderr,
        "Transient timeout on replica node, retrying request"
    );
    let _ = stderr.flush();
    thread::sleep(Duration::from_millis(15));

    let _ = writeln!(stdout, "Processed batch #2 (128 records)");
    let _ = writeln!(stdout, "Worker shutting down cleanly");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn pump_stream_handles_lf_crlf_and_trailing_line_without_newline() {
        let input = b"first line\nsecond line\r\nthird line without newline";
        let mut collected = Vec::new();

        let count = pump_stream(Cursor::new(input), |line| {
            collected.push(line.to_string());
        })
        .expect("pump_stream should succeed on valid in-memory input");

        assert_eq!(count, 3);
        assert_eq!(
            collected,
            vec!["first line", "second line", "third line without newline"]
        );
    }

    #[test]
    fn pump_stream_handles_invalid_utf8_lossily() {
        // 0xFF 0xFE 不是合法 UTF-8 字节序列，应被安全替换为 U+FFFD 而不中断读取
        let input = b"ok-before\n\xff\xfe-invalid\nok-after\n";
        let mut collected = Vec::new();

        let count = pump_stream(Cursor::new(input), |line| {
            collected.push(line.to_string());
        })
        .expect("pump_stream should tolerate invalid UTF-8 bytes");

        assert_eq!(count, 3);
        assert_eq!(collected[0], "ok-before");
        assert!(collected[1].contains('\u{FFFD}'));
        assert_eq!(collected[2], "ok-after");
    }

    #[cfg(unix)]
    #[test]
    fn run_and_log_child_process_captures_stdout_stderr_and_exit_code() {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("echo 'hello stdout 1'; echo 'hello stderr 1' >&2; echo 'hello stdout 2'; exit 0");

        let summary = run_and_log_child_process(&mut cmd, "test_unix_shell")
            .expect("child process execution should succeed");

        assert!(summary.status.success());
        assert_eq!(summary.stdout_lines, 2);
        assert_eq!(summary.stderr_lines, 1);
    }

    #[cfg(unix)]
    #[test]
    fn run_and_log_child_process_reports_nonzero_exit_code() {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg("echo 'failing task' >&2; exit 42");

        let summary = run_and_log_child_process(&mut cmd, "test_unix_failure")
            .expect("waiting for failing child process should succeed");

        assert!(!summary.status.success());
        assert_eq!(summary.status.code(), Some(42));
        assert_eq!(summary.stdout_lines, 0);
        assert_eq!(summary.stderr_lines, 1);
    }
}
