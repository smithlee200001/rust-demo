# 技术栈与基础设施（Tech Stack）

| 层级 | 选型 | 说明 |
| :--- | :--- | :--- |
| 后端语言 / 框架 | TypeScript / Node.js (或 Go / Python) | 统一类型安全与快速迭代 |
| 主数据库 | PostgreSQL 16 | 强一致性、支持行级安全（RLS）做租户隔离 |
| 缓存与会话 | Redis 7 | Token 黑名单、分布式锁、限流计数 |
| 消息队列 | Kafka / NATS | 领域事件异步投递 |
| 观测性 | OpenTelemetry + Prometheus | 全链路 Trace 与指标监控 |
