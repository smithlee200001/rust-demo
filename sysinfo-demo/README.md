# sysinfo-demo：跨平台系统信息采集

[返回项目总览](../README.md)

使用 [`sysinfo`](https://crates.io/crates/sysinfo) 演示跨平台（macOS、Linux、Windows 等）系统信息采集，无需通过子进程调用 `sysctl` 或 `sw_vers` 等平台专用命令。

## 目录结构

```text
sysinfo-demo/
├── README.md
├── Cargo.toml
├── Cargo.lock
└── src/
    └── main.rs        # 按需采集 CPU、内存与操作系统版本信息
```

## 运行方式

安装支持 Edition 2024 的 Rust stable 工具链，在 `sysinfo-demo` 目录下执行：

```bash
cargo run
```

程序将采集结果以英文输出到标准输出后自动退出。首次构建需要下载 `sysinfo` 依赖。

## 示例内容

- **按需刷新**：通过 `System::new_with_specifics` 仅采集所需模块，避免不必要的全量扫描。
- **主机与系统信息**：主机名、系统名称/版本/完整描述、发行版 ID、内核版本、运行时长（`uptime`）、启动时间与系统负载（`load_average`）。
- **CPU 指标**：CPU 品牌型号、厂商 ID、架构、逻辑/物理核心数、基础频率与两次采样计算的实时使用率。
- **内存与 Swap**：总内存、已用内存、可用内存、空闲内存与 Swap 使用情况。
- **磁盘信息**：通过 `Disks` 获取挂载点、文件系统、磁盘类型（SSD/HDD）、可移动属性与可用空间。
- **网络接口**：通过 `Networks` 获取活动网卡的 MAC 地址及累计收发字节数与数据包数。
- **温度传感器**：通过 `Components` 获取硬件温度传感器当前读数与历史最高值（视宿主机权限与硬件支持而定）。
- **进程快照**：获取当前进程信息以及按内存占用排序的 Top 5 进程。

> 若需在 `tokio` 等异步运行时中调用 `collect_system_info()`，建议通过 `tokio::task::spawn_blocking` 执行以避免阻塞异步工作线程。

## 格式化与检查

在 `sysinfo-demo` 目录下执行：

```bash
cargo fmt
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```
