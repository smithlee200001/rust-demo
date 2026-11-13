# log-demo：Rust 日志用法

[返回项目总览](../README.md)

通过自定义 `log::Log` 实现演示 `log` 0.4 的日志级别、动态过滤、日志目标和结构化字段。程序使用模拟数据，日志中的数据库、支付和网络操作不会实际执行。

## 目录结构

```text
log-demo/
├── README.md
├── Cargo.toml
├── Cargo.lock
└── src/
    └── main.rs        # 自定义日志记录器与各类日志示例
```

## 运行方式

安装支持 Edition 2024 的 Rust stable 工具链，在 `log-demo` 目录下执行：

```bash
cargo run
```

程序将英文日志输出到标准输出，包含级别、目标、源码位置和可选键值字段，演示结束后自动退出。首次构建需要下载依赖。

## 示例内容

- `trace!`、`debug!`、`info!`、`warn!` 和 `error!` 五种日志级别。
- 使用 `log!` 根据模拟请求耗时动态选择级别。
- 自定义 `target`，以及子模块默认使用模块路径的行为。
- 使用 `log_enabled!` 避免执行未开启级别的昂贵诊断计算。
- 结构化键值字段，以及 `:?`、`:%` 和 `:serde` 格式。
- 使用 `log::set_max_level` 在运行时切换全局过滤级别。
- `serde_json::Value` 的紧凑、美化、调试和结构化输出。

自定义记录器将结构化字段转换为可读文本，不输出完整 JSON 日志行。过滤级别由代码设置，当前不读取 `RUST_LOG` 环境变量。输出中的警告和错误是演示内容，不代表程序运行失败。

## 格式化与检查

在 `log-demo` 目录下执行：

```bash
cargo fmt
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```
