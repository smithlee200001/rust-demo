# docs-demo：项目文档组织示例

[返回项目总览](../README.md)

本案例演示如何通过总索引、分层文档和历史归档组织项目知识，供团队成员与 AI 编码助手按需查阅。

文档以多租户 SaaS 平台为背景，展示需求、架构、技术设计、接口契约、开发指南、运维和任务管理的分工。这里仅包含文档样例，没有对应的业务实现、Cargo 工程或 Node.js 工程；其中的启动与部署命令用于展示文档写法，不能直接在本目录运行。

## 目录结构

```text
docs-demo/
├── README.md
├── AGENTS.md                # AI 协作入口及高频约束
├── CLAUDE.md                # 引用统一协作入口
└── docs/
    ├── README.md            # 文档总索引
    ├── 01-requirements/     # 业务需求和验收标准
    ├── 02-architecture/     # 全局架构、技术选型及 ADR
    ├── 03-tech-specs/       # 模块设计及 RFC
    ├── 04-contracts/        # API、数据库及事件契约
    ├── 05-guides/           # 开发与测试指南
    ├── 06-operations/       # 部署与故障排查
    └── 07-tasks/            # 当前任务及历史归档
```

从 [文档总索引](./docs/README.md) 阅读所需主题；AI 协作约定见 [AGENTS.md](./AGENTS.md)。归档内容仅供历史参考，不作为当前实现依据。
