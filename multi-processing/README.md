# multi-processing：多进程通信

[返回项目总览](../README.md)

该案例模拟浏览器主进程与多个 Renderer 子进程之间的协作，演示动态创建和关闭子进程、分发任务、接收结果，以及共享内存消息传输。页面渲染通过模拟数据实现，不会实际请求 URL 或渲染网页。

主要使用 `ipc-channel`、Tokio、`prost` 和 Serde。构建脚本通过 `protoc-bin-vendored` 获取 Protobuf 编译器，无需单独安装 `protoc`。

## 目录结构

```text
multi-processing/
├── README.md
├── Cargo.toml
├── Cargo.lock
├── build.rs            # 编译 Protobuf 消息定义
├── proto/              # Protobuf 协议文件
└── src/                # 示例源码；process_support.rs 负责握手超时、错误类型及基准子进程回收
```

## 环境准备

- 安装 Rust stable 工具链（包含 `rustc`、`cargo` 和 `rustfmt`），建议使用最新稳定版。
- 本案例使用 Rust Edition 2024。
- 首次构建需要下载 Cargo 依赖。

## 运行交互式示例

在 `multi-processing` 目录下执行：

```bash
cargo run
```

启动后，在程序控制台输入以下命令：

| 命令 | 说明 |
| --- | --- |
| `spawn` | 创建一个 Renderer 子进程 |
| `list` | 查看子进程编号、PID 和状态 |
| `load <worker_id> <url>` | 向指定子进程发送模拟渲染任务 |
| `load <url>` | 向一个可用子进程发送模拟渲染任务 |
| `kill <worker_id>` | 关闭指定子进程 |
| `help` | 查看命令帮助 |
| `quit` | 关闭子进程和 IPC 线程并退出 |

例如，先输入 `spawn`，等待子进程就绪后输入 `load https://www.rust-lang.org`，查看返回结果，最后使用 `quit` 退出。

标准输入结束（EOF）或读取失败也会触发清理。未连接的工作进程握手等待窗口为 5 秒；已连接的本地工作进程应立即发送握手消息。关闭 Renderer 时先尝试发送关闭指令，发送成功后最多等待 2 秒退出，再尝试强制终止并回收。同步 IPC 发送和进程等待放在阻塞线程池中执行。

IPC、解码和参数错误通过英文错误信息报告。基准测试提前失败时会终止并回收已启动的子进程。该案例面向协作的本地工作进程，握手看门狗不提供针对任意客户端的完整读写超时机制。

## 运行性能对比

在 `multi-processing` 目录下执行：

```bash
cargo run --release -- --bench
```

基准测试比较以下消息传输方式：

| 方案 | 实现 |
| --- | --- |
| 纯字节通道 | 使用 Protobuf 编解码，通过 `ipc::bytes_channel` 传输 |
| 信封 + 字节数组 | 将 Protobuf 字节放入 Serde 信封中传输 |
| 信封 + 共享内存 | 将 Protobuf 字节写入 `IpcSharedMemory`，通过信封传递共享内存句柄 |
| 结构体直接传输 | 为 Protobuf 生成的结构体派生 Serde，通过类型化 IPC 通道传输，实际使用 bincode 序列化 |

测试覆盖 256 B 消息体（5,000 次）和 512 KiB 消息体（200 次）两种场景，输出总耗时、平均每条消息耗时和吞吐量。消息体大小不包含其他字段及序列化开销；结果用于当前环境下的方案比较。

吞吐量以 Protobuf 参考大小按 MiB/s 计算，方案三实际使用 bincode，因此该指标不是物理链路吞吐量。共享内存方案通过句柄共享编码数据，写入共享内存和解码拥有所有权的字段仍有复制，并非端到端零拷贝。

## 格式化与检查

在 `multi-processing` 目录下执行：

```bash
cargo fmt
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```

测试覆盖统计值边界、非法迭代次数、正常握手和未连接超时；运行握手测试需要允许本机 IPC。
