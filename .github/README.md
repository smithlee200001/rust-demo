# 持续集成

[工作流](workflows/ci.yml) 在 PR、`main` 分支 push 和手动触发时运行。开发分支请通过 PR 检查，或在 Actions 页面手动选择分支运行；普通开发分支 push 不重复触发检查。同一 PR 或分支的新运行会取消旧运行。

当前在 Ubuntu 上使用 Rust stable，按 `http-sse` 和 `multi-processing` 两个独立 Cargo 工程并行执行：

```bash
# 先进入对应案例目录。
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Clippy 已覆盖编译检查，因此不额外重复执行 `cargo check` 或 `cargo build`。`--locked` 防止 CI 静默修改依赖版本。IPC 握手测试需要允许本机 IPC；Protobuf 编译使用项目内置的 `protoc-bin-vendored`，不需要安装系统 `protoc`。

缓存按案例隔离，依赖锁文件和 Rust 工具链等变化会影响缓存键；只有 `main` 运行保存缓存，PR 可以恢复缓存。每个案例最多运行 20 分钟，一个失败不会取消另一个。

新增案例时，更新工作流的 `matrix.demo`。当前未声明或验证 Windows、macOS 兼容性，也未启用覆盖率上传服务。
