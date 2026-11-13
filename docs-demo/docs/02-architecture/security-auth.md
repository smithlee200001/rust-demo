# 安全与鉴权模型（Security & Auth）

## 1. 认证方式

- **Access Token**：短生命周期 JWT（15 分钟），仅在内存中持有。
- **Refresh Token**：长生命周期不透明令牌（7 天），通过 `HttpOnly; Secure; SameSite=Lax` Cookie 传递，支持轮换（Rotation）。

## 2. 权限与租户隔离

- 所有业务请求必须携带明确的 `tenant_id` 上下文。
- 禁止在未校验租户归属的情况下直接按主键 `id` 查询或更新资源。
