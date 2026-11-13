# 测试策略（Testing Strategy）

## 测试金字塔

- **单元测试（Unit）**：覆盖所有纯业务逻辑、状态机转换与边界条件（目标覆盖率 >= 80%）。
- **集成测试（Integration）**：使用测试容器（Testcontainers）连接真实 PostgreSQL 验证 SQL、RLS 隔离与事务回滚。
- **契约测试（Contract）**：校验实际接口响应是否符合 `docs/04-contracts/api/openapi.yaml`。

## 运行命令

```bash
npm run test:unit
npm run test:integration
```
