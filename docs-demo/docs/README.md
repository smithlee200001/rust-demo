# 项目文档总索引（Documentation Index）

本目录采用 **“总索引 + 按职责/业务域分层 + 生命周期归档”** 结构，同时服务于团队成员与 AI 编程助手。

## 目录导航

| 目录 | 用途 | 适用场景 |
| :--- | :--- | :--- |
| [01-requirements/](./01-requirements/auth.md) | 纯业务需求、用户场景与验收标准（PRD） | 确认“做什么”与业务边界 |
| [02-architecture/](./02-architecture/overview.md) | 全局架构、技术栈、安全模型与 ADR | 理解系统全貌、做技术选型与跨模块设计 |
| [03-tech-specs/](./03-tech-specs/auth.md) | 模块级详细技术设计（含 RFC） | 确认状态机、数据流、并发/幂等与具体实现方案 |
| [04-contracts/](./04-contracts/database/schema.md) | API、数据库 Schema、异步事件等硬契约 | 前后端联调、接口变更、数据模型调整 |
| [05-guides/](./05-guides/local-setup.md) | 本地搭建、编码规范、测试策略 | 新人上手、AI 遵循项目代码与测试风格 |
| [06-operations/](./06-operations/deployment.md) | 部署流程、环境变量、故障排查手册 | 发布上线、配置环境、排查线上告警 |
| [07-tasks/](./07-tasks/current-sprint.md) | 当前迭代任务追踪与历史归档 | 查看当前进度、拆解任务、查阅历史记录 |

## 文档维护约定

- **单一事实来源**：接口和表结构以 `04-contracts/` 为准，代码变更需同步更新对应文档。
- **生命周期管理**：已上线或已废弃的临时方案与迭代计划，及时移入 `07-tasks/archive/` 并标注 `[ARCHIVED]`。
