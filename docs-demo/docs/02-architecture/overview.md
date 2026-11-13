# 系统整体架构（Architecture Overview）

## 1. 系统定位

本 Demo 模拟一个多租户 SaaS 平台，包含**认证鉴权（Auth）**、**计费订阅（Billing）**与**核心业务服务**。

## 2. 逻辑分层

```text
[Client / Web / CLI]
         │ HTTPS
         ▼
   [API Gateway]  ──► 统一鉴权、限流、请求追踪
         │
   ┌─────┴─────────────┐
   ▼                   ▼
[Auth Service]   [Billing Service]
   │                   │
   └─────┬─────────────┘
         ▼
[PostgreSQL] + [Redis] + [Event Bus]
```

## 3. 核心数据流

1. **同步请求**：客户端携带 JWT 访问网关，网关校验签名与租户上下文后转发至下游服务。
2. **异步解耦**：跨领域状态变更（如“用户注册成功”触发“创建免费订阅”）通过事件总线异步通知，契约见 [domain-events.md](../04-contracts/events/domain-events.md)。
