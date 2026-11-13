# Auth 模块详细技术设计（Tech Spec）

> 对应需求：[auth.md](../01-requirements/auth.md)

## 1. 关键流程（Refresh Token 轮换）

1. 客户端调用 `POST /api/v1/auth/refresh`。
2. 服务端校验 Refresh Token 哈希是否存在于数据库且未过期、未吊销。
3. 若发现已吊销的 Token 被重复使用（疑似泄露），立即吊销该用户当前会话族（Token Family）的所有令牌。
4. 签发新令牌对，并将旧令牌标记为已使用。

## 2. 依赖契约

- API 定义：[openapi.yaml](../04-contracts/api/openapi.yaml)
- 数据表：`users`、`refresh_tokens`，见 [schema.md](../04-contracts/database/schema.md)
