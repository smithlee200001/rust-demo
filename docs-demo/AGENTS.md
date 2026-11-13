# 项目 AI 协作指南（AGENTS.md）

> 本文件作为 AI 编程助手（Codex、Claude Code 等）进入项目时的**第一入口**，仅保留高频约束与文档索引。具体业务和设计细节请按需阅读 `docs/` 下的对应文件，避免一次性加载全量文档。

## 1. 文档导航（渐进式加载）

- **总索引**：[docs/README.md](./docs/README.md)
- **改需求 / 确认验收标准必看**：`docs/01-requirements/`（如 `auth.md`、`billing.md`）
- **改架构 / 选型前必看**：[docs/02-architecture/overview.md](./docs/02-architecture/overview.md) 与 `docs/02-architecture/adr/`
- **改模块详细设计前必看**：`docs/03-tech-specs/`（如 `auth.md`、`billing.md`、`rfcs/`）
- **改接口 / 数据库前必看**：`docs/04-contracts/`
- **当前开发进度**：[docs/07-tasks/current-sprint.md](./docs/07-tasks/current-sprint.md)

## 2. 核心约束

1. **契约优先**：修改 API 或数据库表结构前，必须先更新 `docs/04-contracts/` 并确认向后兼容性。
2. **尊重 ADR**：不得擅自推翻 `docs/02-architecture/adr/` 中已确认的技术决策；如需变更，请先新增一条 ADR 提案。
3. **忽略归档**：`docs/07-tasks/archive/` 中的文档仅作历史留档，禁止作为当前实现依据。
4. **日志与注释**：代码注释使用清晰的中文说明意图与边界；运行日志统一使用英文。
