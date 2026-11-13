//! 基于 `tokio::select!` + 专职 IPC 线程（`ipc-io-thread`）的动态多进程浏览器架构实现
//!
//! 架构设计说明（Tokio 异步运行时 + 专职 IPC 线程混合模型）：
//! 1. **主异步任务（Tokio Main Task / Browser Coordinator）**：
//!    - 运行在 Tokio 异步运行时之上，使用 `tokio::select!` 同时异步监听：
//!      a) `tokio::io::stdin()` 终端异步行读取流（无需再手写独立的 Stdin 阻塞线程）；
//!      b) `tokio::sync::mpsc::UnboundedReceiver`（接收专职 IPC 线程推送过来的跨进程事件）。
//! 2. **专职 IPC 线程（Dedicated `ipc-io-thread`）**：
//!    - 独立操作系统线程，持有内核级多路复用集合 `IpcReceiverSet`。
//!    - 通过 `tokio::sync::mpsc::UnboundedSender`（其 `.send()` 为同步非阻塞方法，支持在普通 OS 线程直接调用）
//!      向 Tokio 主协程实时推送子进程消息（含 Protobuf + `IpcSharedMemory`）与断连事件（`ChannelClosed`）。
//! 3. **异步非阻塞握手（`tokio::task::spawn_blocking`）**：
//!    - 当用户输入 `spawn` 动态拉起新 `Renderer` 子进程时，将阻塞式的 `IpcOneShotServer::accept()`
//!      提交到 Tokio 的 `spawn_blocking` 线程池中执行，避免在主异步任务中等待握手。

use crate::process_support::{DemoResult, accept_worker};
use crate::proto::BenchPayload;
use ipc_channel::ipc::{
    self, IpcOneShotServer, IpcReceiver, IpcReceiverSet, IpcSelectionResult, IpcSender,
    IpcSharedMemory,
};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    io::{self as std_io, Write},
    process::{Child, Command},
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::{
    io::{self as tokio_io, AsyncBufReadExt, BufReader},
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};

// ============================================================================
// 跨进程 IPC 通信协议定义（主进程 <-> Renderer 子进程）
// ============================================================================

/// 主进程发往 Renderer 子进程的控制指令
#[derive(Serialize, Deserialize, Debug)]
pub enum BrowserToRendererCmd {
    /// 握手初始化：下发主进程分配的 `worker_id`，以及子进程专属的事件上报通道发送端
    Init {
        worker_id: u32,
        event_tx: IpcSender<RendererToBrowserEvent>,
    },
    /// 命令子进程渲染指定 URL 并返回共享内存中的 Protobuf 快照
    RenderUrl { seq_id: u64, url: String },
    /// 命令子进程立即优雅退出
    Shutdown,
}

/// Renderer 子进程发往主进程专职 IPC 线程的事件消息
#[derive(Serialize, Deserialize, Debug)]
pub enum RendererToBrowserEvent {
    /// 子进程启动完成并上报操作系统 PID
    Ready { worker_id: u32, pid: u32 },
    /// 子进程已完成页面渲染，携带存放于共享内存中的 Protobuf 数据
    PageRendered {
        worker_id: u32,
        seq_id: u64,
        url: String,
        proto_shm: IpcSharedMemory,
    },
}

// ============================================================================
// 主进程内部通信协议（Tokio 主协程 <-> 专职 IPC 线程）
// ============================================================================

/// Tokio 主协程发往专职 IPC 线程的控制指令（通过唤醒通道发送，支持动态挂载新子进程通道）
#[derive(Serialize, Deserialize)]
enum MainToIpcThreadCtrl {
    /// 动态注册一个新创建的 Renderer 子进程接收端到 `IpcReceiverSet`
    RegisterRenderer {
        worker_id: u32,
        renderer_rx: IpcReceiver<RendererToBrowserEvent>,
    },
    /// 通知专职 IPC 线程退出循环并关闭
    ShutdownIpcThread,
}

/// 专职 IPC 线程上报给 Tokio 主协程的事件（经由 `tokio::sync::mpsc::unbounded_channel` 传递）
#[derive(Debug)]
enum IpcThreadToMainEvent {
    /// IPC 线程无法继续工作，主程序收到后进入统一清理流程。
    IpcThreadFailed { reason: String },
    /// 异步握手完成：新创建的 Renderer 子进程已完成 IPC 连接建立
    HandshakeCompleted {
        worker_id: u32,
        cmd_tx: IpcSender<BrowserToRendererCmd>,
    },
    /// 握手失败（例如子进程启动崩溃）
    HandshakeFailed { worker_id: u32, reason: String },
    /// 收到子进程的 Ready 就绪通知
    RendererReady { worker_id: u32, pid: u32 },
    /// 收到子进程渲染完成的页面数据（已由 IPC 线程从共享内存解码出 Protobuf 摘要）
    RendererPageDone {
        worker_id: u32,
        seq_id: u64,
        url: String,
        title: String,
        shm_bytes: usize,
    },
    /// IPC 线程底层 `IpcReceiverSet` 检测到某个子进程断开连接（正常退出或崩溃）
    RendererDisconnected { worker_id: u32 },
}

/// 终端命令行解析后的用户指令枚举
#[derive(Debug)]
enum TerminalCmd {
    /// 动态创建一个新的 Renderer 子进程
    SpawnRenderer,
    /// 向指定（或默认）Renderer 子进程发送页面渲染请求
    LoadUrl {
        target_worker: Option<u32>,
        url: String,
    },
    /// 查看当前所有存活的 Renderer 子进程列表
    ListRenderers,
    /// 关闭某一个指定的 Renderer 子进程
    KillRenderer { worker_id: u32 },
    /// 打印帮助信息
    Help,
    /// 用户输入了空行（仅重新打印提示符）
    EmptyLine,
    /// 用户输入了无法识别的命令
    Unknown(String),
    /// 主动退出程序；标准输入结束或读取失败也会触发清理
    Quit,
}

/// 主进程维护的活跃 Renderer 子进程元信息
struct RendererProcessInfo {
    /// 子进程操作系统句柄
    child: Child,
    /// 子进程上报的操作系统 PID
    os_pid: Option<u32>,
    /// 向该子进程发送指令的 IPC 通道（`spawn_blocking` 异步握手完成后填充）
    cmd_tx: Option<IpcSender<BrowserToRendererCmd>>,
}

// ============================================================================
// 专职 IPC 线程实现（Dedicated IPC Thread -> Tokio MPSC Channel）
// ============================================================================

/// 启动主进程内部的专职 IPC 线程（`ipc-io-thread`）
///
/// 关键设计：
/// - 专职线程运行同步阻塞的 `IpcReceiverSet::select()`，集中监听所有子进程通道。
/// - 收到子进程事件后，直接调用 `tokio::sync::mpsc::UnboundedSender::send()` 唤醒 Tokio 主协程。
///   因为 `UnboundedSender::send` 是同步非阻塞方法，所以普通 OS 线程无需 Tokio 上下文即可直接发送！
///
/// 参数说明：
/// - `ctrl_rx`: 接收主协程发来的动态注册子进程或关闭指令的 `IpcReceiver`
/// - `event_to_main_tx`: 将 IPC 事件异步推送给 Tokio 主协程的 `UnboundedSender`
fn spawn_dedicated_ipc_thread(
    ctrl_rx: IpcReceiver<MainToIpcThreadCtrl>,
    event_to_main_tx: UnboundedSender<IpcThreadToMainEvent>,
) -> std_io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("ipc-io-thread".into())
        .spawn(move || {
            let mut receiver_set = match IpcReceiverSet::new() {
                Ok(set) => set,
                Err(err) => {
                    let _ = event_to_main_tx.send(IpcThreadToMainEvent::IpcThreadFailed {
                        reason: format!("Failed to initialize receiver set: {err}"),
                    });
                    return;
                }
            };

            // 1. 将主协程的控制唤醒通道加入 IpcReceiverSet，记录其唯一内核 ID (ctrl_set_id)
            let ctrl_set_id = match receiver_set.add(ctrl_rx) {
                Ok(id) => id,
                Err(err) => {
                    let _ = event_to_main_tx.send(IpcThreadToMainEvent::IpcThreadFailed {
                        reason: format!("Failed to register control channel: {err}"),
                    });
                    return;
                }
            };

            // 2. 维护 IpcReceiverSet 内部通道 ID -> 子进程 worker_id 的映射表
            let mut rx_id_to_worker: HashMap<u64, u32> = HashMap::new();

            println!("[IPC-Thread] Dedicated IPC I/O thread started and polling IpcReceiverSet.");

            loop {
                // 阻塞等待任意通道可读（主协程唤醒指令 或 任意子进程发来消息/断开连接）
                let results = match receiver_set.select() {
                    Ok(res) => res,
                    Err(err) => {
                        let _ = event_to_main_tx.send(IpcThreadToMainEvent::IpcThreadFailed {
                            reason: format!("Failed to poll receiver set: {err}"),
                        });
                        break;
                    }
                };

                for event in results {
                    match event {
                        // 情况 A：收到主协程发来的控制指令（通过 ctrl_set_id 识别）
                        IpcSelectionResult::MessageReceived(id, raw_msg) if id == ctrl_set_id => {
                            // 坏消息不能使整个 IPC 线程崩溃；记录错误后继续处理其他通道。
                            let ctrl_cmd = match raw_msg.to::<MainToIpcThreadCtrl>() {
                                Ok(command) => command,
                                Err(err) => {
                                    eprintln!("[IPC-Thread] Invalid control message: {err}");
                                    continue;
                                }
                            };

                            match ctrl_cmd {
                                MainToIpcThreadCtrl::RegisterRenderer {
                                    worker_id,
                                    renderer_rx,
                                } => {
                                    // 动态将新子进程的 IpcReceiver 挂载到内核多路复用集合中
                                    match receiver_set.add(renderer_rx) {
                                        Ok(new_rx_id) => {
                                            rx_id_to_worker.insert(new_rx_id, worker_id);
                                            println!(
                                                "[IPC-Thread] Dynamically registered Renderer #{worker_id} into IpcReceiverSet (channel_id={new_rx_id}, total_monitored={})",
                                                rx_id_to_worker.len()
                                            );
                                        }
                                        Err(e) => {
                                            let _ = event_to_main_tx.send(IpcThreadToMainEvent::HandshakeFailed {
                                                worker_id,
                                                reason: format!("Failed to register renderer receiver: {e}"),
                                            });
                                        }
                                    }
                                }
                                MainToIpcThreadCtrl::ShutdownIpcThread => {
                                    println!(
                                        "[IPC-Thread] Received ShutdownIpcThread signal, terminating IPC thread."
                                    );
                                    return;
                                }
                            }
                        }

                        // 情况 B：收到某个已注册的 Renderer 子进程发来的业务事件
                        IpcSelectionResult::MessageReceived(id, raw_msg) => {
                            if let Some(&worker_id) = rx_id_to_worker.get(&id) {
                                match raw_msg.to::<RendererToBrowserEvent>() {
                                    Ok(RendererToBrowserEvent::Ready { worker_id, pid }) => {
                                        let _ = event_to_main_tx.send(
                                            IpcThreadToMainEvent::RendererReady { worker_id, pid },
                                        );
                                    }
                                    Ok(RendererToBrowserEvent::PageRendered {
                                        worker_id,
                                        seq_id,
                                        url,
                                        proto_shm,
                                    }) => {
                                        // 在专职 IPC 线程中直接完成共享内存的 Protobuf 解码
                                        // 从共享内存读取编码字节；解码后的字段仍需分配与复制。
                                        let decoded = match BenchPayload::decode(&proto_shm[..]) {
                                            Ok(payload) => payload,
                                            Err(err) => {
                                                eprintln!("[IPC-Thread] Invalid Protobuf from Renderer #{worker_id}: {err}");
                                                continue;
                                            }
                                        };
                                        let _ = event_to_main_tx.send(
                                            IpcThreadToMainEvent::RendererPageDone {
                                                worker_id,
                                                seq_id,
                                                url,
                                                title: decoded.title,
                                                shm_bytes: proto_shm.len(),
                                            },
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[IPC-Thread] Error deserializing message from Renderer #{worker_id}: {e:?}"
                                        );
                                    }
                                }
                            }
                        }

                        // 情况 C：某个通道被关闭（子进程正常退出或异常崩溃）
                        IpcSelectionResult::ChannelClosed(id) => {
                            if id == ctrl_set_id {
                                println!(
                                    "[IPC-Thread] Control channel closed, exiting IPC thread."
                                );
                                return;
                            }
                            if let Some(worker_id) = rx_id_to_worker.remove(&id) {
                                println!(
                                    "[IPC-Thread] Detected channel closed for Renderer #{worker_id} (channel_id={id})."
                                );
                                let _ = event_to_main_tx.send(
                                    IpcThreadToMainEvent::RendererDisconnected { worker_id },
                                );
                            }
                        }
                    }
                }
            }
        })
}

/// 使用 `tokio::task::spawn_blocking` 在后台阻塞线程池中完成新子进程的 `IpcOneShotServer` 握手
///
/// 参数说明：
/// - `worker_id`: 新创建的子进程逻辑编号
/// - `server`: 一次性握手服务端实例
/// - `server_name`: 服务端令牌，供未连接超时后的取消唤醒使用
/// - `ipc_ctrl_tx`: 发往专职 IPC 线程的控制通道发送端
/// - `event_to_main_tx`: 发往 Tokio 主协程的事件通道发送端
///
/// 返回阻塞任务句柄，主程序退出前等待其完成；失败通过事件上报。
fn spawn_renderer_handshake_tokio(
    worker_id: u32,
    server: IpcOneShotServer<Option<IpcSender<BrowserToRendererCmd>>>,
    server_name: String,
    ipc_ctrl_tx: IpcSender<MainToIpcThreadCtrl>,
    event_to_main_tx: UnboundedSender<IpcThreadToMainEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || match accept_worker(server, &server_name) {
        Ok(cmd_tx) => {
            // 创建该 Renderer 子进程专属的事件上报通道
            let (event_tx, event_rx) = match ipc::channel::<RendererToBrowserEvent>() {
                Ok(channel) => channel,
                Err(err) => {
                    let _ = event_to_main_tx.send(IpcThreadToMainEvent::HandshakeFailed {
                        worker_id,
                        reason: format!("Failed to create renderer event channel: {err}"),
                    });
                    return;
                }
            };

            // 1. 先将该子进程的接收端 event_rx 发送给专职 IPC 线程，动态挂载进 IpcReceiverSet
            if let Err(e) = ipc_ctrl_tx.send(MainToIpcThreadCtrl::RegisterRenderer {
                worker_id,
                renderer_rx: event_rx,
            }) {
                let _ = event_to_main_tx.send(IpcThreadToMainEvent::HandshakeFailed {
                    worker_id,
                    reason: format!("Failed to register with IPC thread: {e}"),
                });
                return;
            }

            // 2. 向子进程发送 Init 指令，把事件上报发送端 event_tx 交给子进程
            if let Err(e) = cmd_tx.send(BrowserToRendererCmd::Init {
                worker_id,
                event_tx,
            }) {
                let _ = event_to_main_tx.send(IpcThreadToMainEvent::HandshakeFailed {
                    worker_id,
                    reason: format!("Failed to send Init to child: {e}"),
                });
                return;
            }

            // 3. 将握手成功的指令发送端 cmd_tx 交还给 Tokio 主协程保存
            let _ = event_to_main_tx
                .send(IpcThreadToMainEvent::HandshakeCompleted { worker_id, cmd_tx });
        }
        Err(e) => {
            let _ = event_to_main_tx.send(IpcThreadToMainEvent::HandshakeFailed {
                worker_id,
                reason: format!("IpcOneShotServer accept failed: {e}"),
            });
        }
    })
}

// ============================================================================
// 终端命令解析与帮助菜单辅助函数
// ============================================================================

/// 解析用户在终端输入的单行命令字符串
fn parse_terminal_command(line: &str) -> TerminalCmd {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return TerminalCmd::EmptyLine;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    match parts[0].to_ascii_lowercase().as_str() {
        "spawn" | "new" | "create" => TerminalCmd::SpawnRenderer,
        "list" | "ls" | "status" => TerminalCmd::ListRenderers,
        "help" | "h" | "?" => TerminalCmd::Help,
        "quit" | "exit" | "q" => TerminalCmd::Quit,
        "kill" | "stop" | "close" => {
            if parts.len() >= 2
                && let Ok(worker_id) = parts[1].parse::<u32>()
            {
                return TerminalCmd::KillRenderer { worker_id };
            }
            TerminalCmd::Unknown("Usage: kill <worker_id> (e.g., `kill 0`)".into())
        }
        "load" | "render" | "open" => {
            if parts.len() == 2 {
                TerminalCmd::LoadUrl {
                    target_worker: None,
                    url: parts[1].to_string(),
                }
            } else if parts.len() >= 3
                && let Ok(worker_id) = parts[1].parse::<u32>()
            {
                TerminalCmd::LoadUrl {
                    target_worker: Some(worker_id),
                    url: parts[2..].join(" "),
                }
            } else {
                TerminalCmd::Unknown(
                    "Usage: load <worker_id> <url> OR load <url> (e.g., `load 0 https://servo.org`)"
                        .into(),
                )
            }
        }
        _ => TerminalCmd::Unknown(format!(
            "Unknown command: '{trimmed}'. Type 'help' for available commands."
        )),
    }
}

/// 打印交互式命令行帮助菜单
fn print_help_banner() {
    println!(
        "\n=========================================================================================="
    );
    println!("  Interactive Multi-Process Browser (Tokio::select! + Dedicated IPC Thread)");
    println!(
        "=========================================================================================="
    );
    println!("  Available Terminal Commands:");
    println!("    spawn                       - Dynamically create a new Renderer child process");
    println!(
        "    load <worker_id> <url>      - Send URL render task to a specific Renderer (e.g. `load 0 https://rust-lang.org`)"
    );
    println!("    load <url>                  - Send URL render task to any available Renderer");
    println!("    list                        - Show all active Renderer child processes and PIDs");
    println!(
        "    kill <worker_id>            - Gracefully stop a specific Renderer process (e.g. `kill 0`)"
    );
    println!("    help                        - Show this command help message");
    println!(
        "    quit                        - Shut down IPC thread, stop all Renderers, and exit"
    );
    println!(
        "=========================================================================================="
    );
}

/// 打印命令行提示符并刷新标准输出缓冲区
fn print_prompt() {
    print!("browser> ");
    if let Err(err) = std_io::stdout().flush() {
        eprintln!("Failed to flush terminal prompt: {err}");
    }
}

// ============================================================================
// Tokio 主事件循环（`#[tokio::main]` + `tokio::select!`）
// ============================================================================

/// 启动交互式主程序；初始化失败返回错误，quit 或 EOF 时回收子进程、握手任务与 IPC 线程。
#[tokio::main]
pub async fn run_interactive_browser() -> DemoResult<()> {
    print_help_banner();

    let current_exe = env::current_exe()?;

    // 1. 创建主协程与专职 IPC 线程之间的控制唤醒通道（用于动态挂载新创建的 Renderer 通道）
    let (ipc_ctrl_tx, ipc_ctrl_rx) = ipc::channel::<MainToIpcThreadCtrl>()?;

    // 2. 创建专职 IPC 线程向 Tokio 主协程汇报事件的异步无界通道
    let (ipc_event_tx, mut ipc_event_rx): (
        UnboundedSender<IpcThreadToMainEvent>,
        UnboundedReceiver<IpcThreadToMainEvent>,
    ) = mpsc::unbounded_channel();

    // 3. 启动专职 IPC 线程（ipc-io-thread）
    let ipc_thread_handle = spawn_dedicated_ipc_thread(ipc_ctrl_rx, ipc_event_tx.clone())?;

    // 4. 创建 Tokio 异步标准输入行读取器（无需再创建单独的 Stdin 阻塞线程）
    let mut stdin_lines = BufReader::new(tokio_io::stdin()).lines();

    // 5. 主协程维护的动态子进程状态表与单调递增序号
    let mut active_renderers: HashMap<u32, RendererProcessInfo> = HashMap::new();
    let mut next_worker_id: u32 = 0;
    let mut next_seq_id: u64 = 1;
    // 保存握手任务，以便退出前等待其完成或触发未连接超时。
    let mut handshake_tasks = Vec::new();
    // 致命 IPC 错误在清理完成后返回，避免误报正常退出。
    let mut failure = None;

    print_prompt();

    // 6. 使用 tokio::select! 同时异步监听【终端用户输入流】和【专职 IPC 线程事件流】
    loop {
        tokio::select! {
            // ----------------------------------------------------------------
            // 分支一：异步读取用户在 Terminal 输入的一行命令
            // ----------------------------------------------------------------
            line_result = stdin_lines.next_line() => {
                let line = match line_result {
                    Ok(Some(l)) => l,
                    Ok(None) => {
                        // 标准输入遇到 EOF 时自动执行优雅退出
                        println!("\n[Tokio-Main] Stdin EOF reached. Shutting down...");
                        break;
                    }
                    Err(e) => {
                        eprintln!("\n[Tokio-Main] Error reading from stdin: {e}. Shutting down...");
                        failure = Some(e);
                        break;
                    }
                };

                match parse_terminal_command(&line) {
                    TerminalCmd::SpawnRenderer => {
                        let worker_id = next_worker_id;
                        let Some(next_id) = next_worker_id.checked_add(1) else {
                            eprintln!("[Tokio-Main] Renderer ID space exhausted.");
                            print_prompt();
                            continue;
                        };
                        next_worker_id = next_id;

                        match IpcOneShotServer::<Option<IpcSender<BrowserToRendererCmd>>>::new() {
                            Ok((server, server_name)) => {
                                match Command::new(&current_exe)
                                    .arg("--mux-renderer")
                                    .arg(&server_name)
                                    .spawn()
                                {
                                    Ok(child) => {
                                        let spawned_pid = child.id();
                                        println!(
                                            "[Tokio-Main] Spawning Renderer #{worker_id} (OS PID: {spawned_pid})... Handshake delegated to tokio::spawn_blocking."
                                        );
                                        active_renderers.insert(
                                            worker_id,
                                            RendererProcessInfo {
                                                child,
                                                os_pid: Some(spawned_pid),
                                                cmd_tx: None,
                                            },
                                        );
                                        // 使用 tokio::task::spawn_blocking 异步执行阻塞握手，避免阻塞运行时线程
                                        handshake_tasks.push(spawn_renderer_handshake_tokio(
                                            worker_id,
                                            server,
                                            server_name,
                                            ipc_ctrl_tx.clone(),
                                            ipc_event_tx.clone(),
                                        ));
                                    }
                                    Err(e) => {
                                        eprintln!("[Tokio-Main] Error spawning child process: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[Tokio-Main] Error creating IpcOneShotServer: {e}");
                            }
                        }
                        print_prompt();
                    }

                    TerminalCmd::LoadUrl { target_worker, url } => {
                        let chosen_worker = match target_worker {
                            Some(id) => Some(id),
                            None => active_renderers
                                .iter()
                                .find(|(_, info)| info.cmd_tx.is_some())
                                .map(|(&id, _)| id),
                        };

                        match chosen_worker {
                            Some(worker_id) => {
                                if let Some(info) = active_renderers.get(&worker_id) {
                                    if let Some(ref cmd_tx) = info.cmd_tx {
                                        let seq_id = next_seq_id;
                                        let Some(next_id) = next_seq_id.checked_add(1) else {
                                            eprintln!("[Tokio-Main] Render sequence ID space exhausted.");
                                            print_prompt();
                                            continue;
                                        };
                                        next_seq_id = next_id;
                                        println!(
                                            "[Tokio-Main] Sending RenderUrl(seq={seq_id}, url='{url}') to Renderer #{worker_id}"
                                        );
                                        // IPC 发送可能因背压阻塞，在阻塞线程池中执行。
                                        let sender = cmd_tx.clone();
                                        match tokio::task::spawn_blocking(move || sender.send(BrowserToRendererCmd::RenderUrl { seq_id, url })).await {
                                            Ok(Ok(())) => {}
                                            result => eprintln!("[Tokio-Main] Failed to send command to Renderer #{worker_id}: {result:?}"),
                                        }
                                    } else {
                                        println!("[Tokio-Main] Renderer #{worker_id} is still handshaking, please wait a moment.");
                                    }
                                } else {
                                    println!(
                                        "[Tokio-Main] Renderer #{worker_id} does not exist. Type 'spawn' to create one or 'list' to view active renderers."
                                    );
                                }
                            }
                            None => {
                                println!(
                                    "[Tokio-Main] No active Renderer processes available! Type 'spawn' first to create a Renderer."
                                );
                            }
                        }
                        print_prompt();
                    }

                    TerminalCmd::ListRenderers => {
                        if active_renderers.is_empty() {
                            println!("[Tokio-Main] Active Renderers: 0 (Type 'spawn' to create one)");
                        } else {
                            println!("[Tokio-Main] Active Renderers ({}):", active_renderers.len());
                            let mut ids: Vec<u32> = active_renderers.keys().copied().collect();
                            ids.sort_unstable();
                            for id in ids {
                                let info = &active_renderers[&id];
                                let status = if info.cmd_tx.is_some() { "READY" } else { "HANDSHAKING" };
                                println!(
                                    "  - Renderer #{id} | PID: {:?} | Status: {status}",
                                    info.os_pid.unwrap_or(0)
                                );
                            }
                        }
                        print_prompt();
                    }

                    TerminalCmd::KillRenderer { worker_id } => {
                        if let Some(info) = active_renderers.remove(&worker_id) {
                            stop_renderer(worker_id, info, true).await;
                        } else {
                            println!("[Tokio-Main] Renderer #{worker_id} not found.");
                        }
                        print_prompt();
                    }

                    TerminalCmd::Help => {
                        print_help_banner();
                        print_prompt();
                    }

                    TerminalCmd::EmptyLine => {
                        print_prompt();
                    }

                    TerminalCmd::Unknown(msg) => {
                        println!("[Tokio-Main] {msg}");
                        print_prompt();
                    }

                    TerminalCmd::Quit => {
                        println!(
                            "\n[Tokio-Main] 'quit' received. Shutting down {} active Renderer(s) and dedicated IPC thread...",
                            active_renderers.len()
                        );
                        break;
                    }
                }
            }

            // ----------------------------------------------------------------
            // 分支二：异步接收专职 IPC 线程（ipc-io-thread）推送过来的跨进程事件
            // ----------------------------------------------------------------
            Some(event) = ipc_event_rx.recv() => {
                println!();
                match event {
                    IpcThreadToMainEvent::IpcThreadFailed { reason } => {
                        failure = Some(std_io::Error::other(reason));
                        break;
                    }
                    IpcThreadToMainEvent::HandshakeCompleted { worker_id, cmd_tx } => {
                        if let Some(info) = active_renderers.get_mut(&worker_id) {
                            info.cmd_tx = Some(cmd_tx);
                            println!("[Tokio-Main] Handshake completed for Renderer #{worker_id}. Channel bound to IPC thread.");
                        }
                    }
                    IpcThreadToMainEvent::HandshakeFailed { worker_id, reason } => {
                        eprintln!("[Tokio-Main] Handshake failed for Renderer #{worker_id}: {reason}");
                        if let Some(info) = active_renderers.remove(&worker_id) {
                            stop_renderer(worker_id, info, false).await;
                        }
                    }
                    IpcThreadToMainEvent::RendererReady { worker_id, pid } => {
                        if let Some(info) = active_renderers.get_mut(&worker_id) {
                            info.os_pid = Some(pid);
                        }
                        println!("[Tokio-Main] <<< IPC Event: Renderer #{worker_id} (PID: {pid}) is READY to accept commands!");
                    }
                    IpcThreadToMainEvent::RendererPageDone {
                        worker_id,
                        seq_id,
                        url,
                        title,
                        shm_bytes,
                    } => {
                        println!(
                            "[Tokio-Main] <<< IPC Event: Renderer #{worker_id} finished seq #{seq_id} ({url}) -> Protobuf Title: \"{title}\", Shared Memory Size: {shm_bytes} bytes"
                        );
                    }
                    IpcThreadToMainEvent::RendererDisconnected { worker_id } => {
                        if let Some(info) = active_renderers.remove(&worker_id) {
                            stop_renderer(worker_id, info, false).await;
                            println!(
                                "[Tokio-Main] <<< IPC Event: Renderer #{worker_id} disconnected and OS process reaped. Remaining Renderers: {}",
                                active_renderers.len()
                            );
                        }
                    }
                }
                print_prompt();
            }
        }
    }

    // 7. 回收进程与握手任务；未连接的工作进程最多等待握手看门狗的五秒窗口。
    for (worker_id, info) in active_renderers.drain() {
        stop_renderer(worker_id, info, true).await;
    }
    for task in handshake_tasks {
        if let Err(err) = task.await {
            eprintln!("[Tokio-Main] Handshake task failed: {err}");
        }
    }

    // 8. 阻塞的发送和线程 join 放入阻塞池；先关闭所有控制发送端，确保 IPC 线程可退出。
    drop(ipc_event_tx);
    tokio::task::spawn_blocking(move || {
        if let Err(err) = ipc_ctrl_tx.send(MainToIpcThreadCtrl::ShutdownIpcThread) {
            eprintln!("[Tokio-Main] Failed to send IPC shutdown: {err}");
        }
        drop(ipc_ctrl_tx);
        if ipc_thread_handle.join().is_err() {
            eprintln!("[Tokio-Main] IPC thread panicked.");
        }
    })
    .await?;
    if let Some(err) = failure {
        return Err(err.into());
    }
    println!("[Tokio-Main] Dedicated IPC thread joined. Program exited cleanly.");
    Ok(())
}

/// 停止并回收一个 Renderer，所有同步 IPC 与进程等待都放入阻塞池。
///
/// `worker_id` 用于日志，`info` 转移进程所有权；`graceful` 为真时先发 Shutdown，
/// 否则立即终止。收到关闭请求后最多等待两秒，再强制终止并 wait，避免遗留僵尸进程。
async fn stop_renderer(worker_id: u32, mut info: RendererProcessInfo, graceful: bool) {
    let result = tokio::task::spawn_blocking(move || -> std_io::Result<()> {
        let requested = graceful
            && info
                .cmd_tx
                .as_ref()
                .is_some_and(|tx| tx.send(BrowserToRendererCmd::Shutdown).is_ok());
        if requested {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while std::time::Instant::now() < deadline {
                if info.child.try_wait()?.is_some() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        if info.child.try_wait()?.is_none() {
            info.child.kill()?;
        }
        info.child.wait()?;
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => println!("[Tokio-Main] Renderer #{worker_id} reaped."),
        error => eprintln!("[Tokio-Main] Failed to stop Renderer #{worker_id}: {error:?}"),
    }
}

// ============================================================================
// Renderer 子进程实现（由主进程动态拉起）
// ============================================================================

/// 渲染子进程（Renderer Process）入口函数
///
/// 参数说明：
/// - `server_name`: 主进程 `IpcOneShotServer` 生成的握手令牌字符串
///
/// 连接、初始化和发送失败返回错误；主进程关闭命令通道时正常结束。
pub fn run_mux_renderer_worker(server_name: &str) -> DemoResult<()> {
    let bootstrap_tx = IpcSender::connect(server_name.to_string())?;
    let (cmd_tx, cmd_rx) = ipc::channel::<BrowserToRendererCmd>()?;
    bootstrap_tx.send(Some(cmd_tx))?;

    // 1. 等待主进程下发 Init 消息（获取 worker_id 与事件上报通道 event_tx）
    let (worker_id, event_tx) = match cmd_rx.recv()? {
        BrowserToRendererCmd::Init {
            worker_id,
            event_tx,
        } => (worker_id, event_tx),
        other => {
            return Err(std_io::Error::other(format!(
                "Unexpected first renderer message: {other:?}"
            ))
            .into());
        }
    };

    // 2. 向主进程专职 IPC 线程上报 Ready 事件
    event_tx.send(RendererToBrowserEvent::Ready {
        worker_id,
        pid: std::process::id(),
    })?;

    // 3. 子进程命令处理循环
    loop {
        // 主进程正常关闭通道时退出；坏消息或底层 I/O 故障仍应向入口返回错误。
        let cmd = match cmd_rx.recv() {
            Ok(command) => command,
            Err(ipc::IpcError::Disconnected) => break,
            Err(err) => return Err(err.into()),
        };
        match cmd {
            BrowserToRendererCmd::RenderUrl { seq_id, url } => {
                // 模拟异步页面解析耗时
                thread::sleep(Duration::from_millis(10));

                // 构造 Protobuf 页面快照并写入共享内存 IpcSharedMemory
                let payload = BenchPayload {
                    seq_id,
                    tab_id: worker_id,
                    url: url.clone(),
                    title: format!("Rendered Page [{url}] by Renderer #{worker_id}"),
                    headers: HashMap::new(),
                    body_data: format!(
                        "<html><head><title>{url}</title></head><body>Rendered by Worker #{worker_id} (PID: {})</body></html>",
                        std::process::id()
                    )
                    .into_bytes(),
                };
                let shm = IpcSharedMemory::from_bytes(&payload.encode_to_vec());

                event_tx.send(RendererToBrowserEvent::PageRendered {
                    worker_id,
                    seq_id,
                    url,
                    proto_shm: shm,
                })?;
            }
            BrowserToRendererCmd::Shutdown => {
                println!(
                    "[Renderer #{worker_id} (PID: {})] Received Shutdown command, exiting process.",
                    std::process::id()
                );
                break;
            }
            BrowserToRendererCmd::Init { .. } => {
                return Err(std_io::Error::other("Unexpected repeated Init message").into());
            }
        }
    }
    Ok(())
}
