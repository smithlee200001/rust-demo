# 异步领域事件契约（Domain Events）

所有事件均遵循统一信封结构：

```json
{
  "event_id": "evt_01J9...",
  "event_type": "auth.user.registered",
  "version": "1.0",
  "occurred_at": "2026-11-13T06:00:00Z",
  "tenant_id": "ten_123",
  "payload": {}
}
```

## 已注册事件

| 事件类型 | 生产者 | 消费者 | Payload 关键字段 |
| :--- | :--- | :--- | :--- |
| `auth.user.registered` | `auth` | `billing`, `notification` | `user_id`, `email` |
| `billing.subscription.upgraded` | `billing` | `auth` (刷新权限缓存) | `tenant_id`, `plan` |
