# Rust Demo

用于收集和实践各种 Rust 技术的示例工程。每个案例按主题放在独立目录中，方便单独阅读、运行和扩展；后续会持续加入更多 Rust 相关的 demo。

## 项目结构

```text
rust-demo/
├── README.md              # 工程总览与案例索引
├── AGENTS.md              # 仓库统一的 AI 协作与 Rust 开发规范
├── .gitignore             # 通用 Git 忽略规则
├── LICENSE                # MIT 许可证
├── docs-demo/             # 项目文档组织示例
├── http-sse/              # HTTP/SSE 实时推送案例，包含独立的 README 和 Cargo 工程
├── log-demo/              # 日志用法案例，包含独立的 README 和 Cargo 工程
└── multi-processing/      # 多进程通信案例，包含独立的 README 和 Cargo 工程
```

每个案例目录维护自己的 README，具体介绍、环境要求及使用方式见对应目录的文档。当前仓库根目录没有 Cargo workspace 配置，Rust 代码案例作为独立 Cargo 工程管理；文档类案例单独组织，不要求包含 Cargo 工程。

AI 编码助手的统一协作要求见 [AGENTS.md](./AGENTS.md)。

## 案例索引

| 案例 | 主题 |
| --- | --- |
| [docs-demo](./docs-demo/README.md) | 项目文档分层、AI 协作入口与生命周期管理 |
| [http-sse](./http-sse/README.md) | HTTP/SSE 实时指标推送与模拟流式对话 |
| [log-demo](./log-demo/README.md) | 日志级别、结构化字段与自定义日志记录器 |
| [multi-processing](./multi-processing/README.md) | 多进程与进程间通信（IPC） |

新增案例时，在根目录下创建对应的主题目录，并在此索引中添加文档入口。

## 许可证

本项目使用 [MIT License](./LICENSE)。
