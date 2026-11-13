# Billing 模块详细技术设计（Tech Spec）

> 对应需求：[billing.md](../01-requirements/billing.md)

## 1. 状态机

```text
trialing ──► active ──► past_due ──► canceled
```

## 2. 幂等性设计

- 接收支付网关 Webhook 时，使用 `gateway_event_id` 作为唯一键写入 `webhook_events` 表。
- 仅当插入成功时才推进订阅状态并更新账单。
