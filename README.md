# Rust Demo

用于收集和实践各种 Rust 技术的示例工程。每个案例按主题放在独立目录中，方便单独阅读、运行和扩展；后续会持续加入更多 Rust 相关的 demo。

## 项目结构

```text
rust-demo/
├── README.md              # 工程总览与案例索引
├── AGENTS.md              # 仓库统一的 AI 协作与 Rust 开发规范
├── .gitignore             # 通用 Git 忽略规则
├── LICENSE                # MIT 许可证
├── http-sse/              # HTTP/SSE 实时推送案例，包含独立的 README 和 Cargo 工程
└── multi-processing/      # 多进程通信案例，包含独立的 README 和 Cargo 工程
```

每个案例目录维护自己的源码、依赖配置和 README，具体介绍、环境要求及运行方式见对应目录的文档。当前仓库根目录没有 Cargo workspace 配置，各案例作为独立 Cargo 工程管理。

AI 编码助手的统一协作要求见 [AGENTS.md](./AGENTS.md)。

## 案例索引

| 案例 | 主题 |
| --- | --- |
| [http-sse](./http-sse/README.md) | HTTP/SSE 实时指标推送与模拟流式对话 |
| [multi-processing](./multi-processing/README.md) | 多进程与进程间通信（IPC） |

新增案例时，在根目录下创建对应的主题目录，并在此索引中添加文档入口。

## 许可证

本项目使用 [MIT License](./LICENSE)。
