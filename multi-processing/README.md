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
└── src/                # 示例源码
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

## 格式化与检查

在 `multi-processing` 目录下执行：

```bash
cargo fmt
cargo check
```
