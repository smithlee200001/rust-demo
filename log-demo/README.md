# log-demo：Rust 日志用法

[返回项目总览](../README.md)

通过自定义 `log::Log` 实现演示 `log` 0.4 的日志级别、动态过滤、日志目标和结构化字段。程序使用模拟数据，日志中的数据库、支付和网络操作不会实际执行。

## 目录结构

```text
log-demo/
├── README.md
├── Cargo.toml
├── Cargo.lock
├── tests/file_cli.rs      # 文件模式和子进程端到端测试
└── src/
    ├── main.rs            # 自定义日志记录器与各类日志示例
    ├── child_process.rs   # 子进程 stdout/stderr 并发接管与日志路由
    ├── file_log.rs        # 顺序写入、文件加密和收尾
    ├── redact.rs          # 常见主目录路径的用户名掩码
    └── bin/log-file-bench.rs
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
- 接管子进程的 `stdout` 和 `stderr`：通过 `std::thread::scope` 并发读取管道并路由到 `child::stdout`（`INFO`）与 `child::stderr`（`WARN`），附带 `child`、`pid` 等结构化字段。

自定义记录器将结构化字段转换为可读文本，不输出完整 JSON 日志行。过滤级别由代码设置，当前不读取 `RUST_LOG` 环境变量。输出中的警告和错误是演示内容，不代表程序运行失败。

## 保存文件：明文、对称或非对称加密

在 `log-demo` 目录执行。只要指定 `--file`，演示日志只写入新文件，不再镜像到控制台。每次运行须用尚不存在的文件名，不覆盖、不追加；加密失败不改写明文。

```bash
# 明文文件（默认开启用户名脱敏）
cargo run --locked -- --file demo.log

# 对称：终端无回显提示输入高强度独立口令
cargo run --locked -- --file symmetric.age --encryption symmetric

# 非对称：只把 age 公钥给写日志的机器；私钥留在解密端
age-keygen -o reader-key.txt
age-keygen -y reader-key.txt
cargo run --locked -- --file recipient.age --encryption asymmetric --recipient age1REPLACE_WITH_YOUR_PUBLIC_KEY
```

后两种均为 [age 文件格式](https://docs.rs/age/0.12.1/age/)：对称模式用 scrypt 从口令派生保护密钥；非对称模式用 X25519 收件人公钥保护文件密钥。两种模式用同一类分块认证加密保护文件内容；公钥加密不等于数字签名。使用 [age CLI](https://github.com/FiloSottile/age) 解密：`age -d symmetric.age` 或 `age -d -i reader-key.txt recipient.age`。这些命令会把解密结果输出到终端；如需另存明文，应先确认目标位置的访问权限。不要把真实口令或私钥放进命令参数、源码、日志或其他人可读的位置。

- 文件模式默认在加密前统一脱敏，包括日志正文、格式化后的结构化字段、源码位置和父进程采集的子进程日志。举例：`/home/alice/data` → `/home/***/data`，`/Users/alice/data` → `/Users/***/data`，`C:\Users\alice\data` → `C:\Users\***\data`。也识别常见 JSON/Debug 的双反斜杠表示。仅在用合成数据比较时建议使用 `--no-redact`。
- 这是常见绝对主目录路径规则，不识别任意秘密、用户名在路径外的出现、自定义主目录、编码后的路径或所有标点形式。请不要向日志传入密码、令牌等秘密。
- Unix 新文件请求权限 `0600`。Windows 继承日志目录访问权限。文件以 UTF-8 文本或二进制 age 流写入，记录中的换行用转义表示；不是 JSON Lines。
- 正常退出时显式完成加密流并刷新缓冲区；强退可能留下不完整的加密文件。本例不执行 fsync，不保证断电持久化，也不提供日志轮转或跨进程追加。

## 性能测试

[查看本次 Linux 实测和数据解读](BENCHMARK.md)。

```bash
cargo run --locked --release --bin log-file-bench -- --records 10000 --rounds 5
```

同一批合成日志，比较明文、仅脱敏、对称加密、对称加密＋脱敏、非对称加密、非对称加密＋脱敏。输出创建/密钥初始化、批量写入、完成及总耗时的中位数、相对明文的倍数和文件大小。初始化包含 scrypt 或 X25519 的每文件操作，不包含终端输入口令、外部密钥生成；写入不测试终端输出、并发锁争用或 fsync。基准每轮使用独立临时文件，不读取项目或用户日志。临时目录所在文件系统、CPU、口令派生成本和记录长度都会影响结果。以发行版模式复测，首轮无预热。

## 格式化与检查

在 `log-demo` 目录下执行：

```bash
cargo fmt
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```
