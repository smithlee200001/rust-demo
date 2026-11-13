//! 子进程公共支持：统一错误返回，并在基准测试提前失败时回收已启动的子进程。

use std::{
    env,
    error::Error,
    io,
    process::{Child, Command},
    sync::mpsc,
    thread,
    time::Duration,
};

use ipc_channel::ipc::{IpcOneShotServer, IpcSender};
use serde::{Deserialize, Serialize};

/// 示例的统一结果类型；允许在普通线程与 Tokio 阻塞任务之间传递错误。
pub type DemoResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// 等待本地工作进程的首条握手消息；若其未连接，在五秒后发送取消消息唤醒 accept。
///
/// `server` 与 `server_name` 必须来自同一次创建，工作进程连接后必须立即发送 `Some`。
/// `None` 专用于取消；该超时针对未连接的本地工作进程，不是任意恶意客户端的读超时。
/// 函数阻塞，异步调用方必须放入 `spawn_blocking`；返回前会回收看门狗线程。
pub fn accept_worker<T>(server: IpcOneShotServer<Option<T>>, server_name: &str) -> DemoResult<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let (stop_tx, stop_rx) = mpsc::channel();
    thread::scope(|scope| {
        let watchdog = scope.spawn(move || {
            if matches!(
                stop_rx.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                // 正常握手若恰好完成，命名端点可能已经关闭，此时无需再次唤醒。
                if let Ok(sender) = IpcSender::<Option<T>>::connect(server_name.to_owned()) {
                    let _ = sender.send(None);
                }
            }
        });
        let accepted = server.accept();
        let _ = stop_tx.send(());
        watchdog
            .join()
            .map_err(|_| io::Error::other("Handshake watchdog panicked"))?;
        let (_, worker) = accepted?;
        worker.ok_or_else(|| {
            io::Error::new(io::ErrorKind::TimedOut, "Worker handshake timed out").into()
        })
    })
}

/// 持有基准测试子进程，防止握手、编解码或通信失败后遗留运行中的进程。
pub struct BenchChild(Child);

impl BenchChild {
    /// 启动当前可执行文件的工作进程。
    ///
    /// `mode` 指定方案，`server_name` 是握手令牌，`iterations` 必须大于零。
    /// 无效次数、可执行文件定位失败或进程创建失败时返回错误。
    pub fn spawn(mode: &str, server_name: &str, iterations: usize) -> DemoResult<Self> {
        if iterations == 0 {
            return Err(
                io::Error::new(io::ErrorKind::InvalidInput, "Iterations must be positive").into(),
            );
        }
        let child = Command::new(env::current_exe()?)
            .args([mode, server_name, &iterations.to_string()])
            .spawn()
            .map_err(|err| io::Error::new(err.kind(), format!("Failed to spawn {mode}: {err}")))?;
        Ok(Self(child))
    }

    /// 等待子进程退出并验证退出状态；非零退出码作为基准测试错误返回。
    pub fn wait_success(&mut self) -> DemoResult<()> {
        let status = self.0.wait()?;
        if !status.success() {
            return Err(io::Error::other(format!("Benchmark worker exited with {status}")).into());
        }
        Ok(())
    }
}

impl Drop for BenchChild {
    /// 错误返回时终止尚未退出的进程并回收；析构中只记录错误，避免二次 panic。
    fn drop(&mut self) {
        match self.0.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if let Err(err) = self.0.kill() {
                    eprintln!("Failed to terminate benchmark worker: {err}");
                }
            }
            Err(err) => {
                eprintln!("Failed to query benchmark worker: {err}");
                if let Err(err) = self.0.kill() {
                    eprintln!("Failed to terminate benchmark worker: {err}");
                }
            }
        }
        if let Err(err) = self.0.wait() {
            eprintln!("Failed to reap benchmark worker: {err}");
        }
    }
}

/// 握手回归测试，验证正常消息传递及工作进程未连接时的退出路径。
#[cfg(test)]
mod tests {
    use super::*;

    /// 本地发送端完成握手后，应直接返回消息，不等待看门狗超时。
    #[test]
    fn handshake_returns_worker_message() -> DemoResult<()> {
        let (server, name) = IpcOneShotServer::<Option<u32>>::new()?;
        let sender = IpcSender::connect(name.clone())?;
        sender.send(Some(42u32))?;
        assert_eq!(accept_worker(server, &name)?, 42);
        Ok(())
    }

    /// 没有工作进程连接时，取消消息应唤醒 accept 并返回超时，而不是永久阻塞。
    #[test]
    fn handshake_times_out_without_worker() -> DemoResult<()> {
        let (server, name) = IpcOneShotServer::<Option<u32>>::new()?;
        let error = accept_worker(server, &name).expect_err("Missing worker should time out");
        assert_eq!(
            error.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::TimedOut)
        );
        Ok(())
    }

    /// 零迭代在启动进程前就返回错误，不允许创建无法统计的基准任务。
    #[test]
    fn benchmark_rejects_zero_iterations() {
        assert!(BenchChild::spawn("--worker-scheme1", "unused", 0).is_err());
    }
}
