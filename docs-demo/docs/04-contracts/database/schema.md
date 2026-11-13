# 数据库 Schema 与迁移规范

## 1. 核心表概览

### `users`
| 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `id` | `uuid` (PK) | 用户唯一标识 |
| `tenant_id` | `uuid` (FK, NOT NULL) | 所属租户 ID |
| `email` | `varchar(255)` | 唯一邮箱（租户内或全局唯一） |
| `password_hash` | `text` | Argon2id 哈希 |
| `created_at` | `timestamptz` | 创建时间 |

### `subscriptions`
| 字段 | 类型 | 说明 |
| :--- | :--- | :--- |
| `id` | `uuid` (PK) | 订阅 ID |
| `tenant_id` | `uuid` (UNIQUE, NOT NULL) | 租户 ID |
| `plan` | `varchar(32)` | `free` / `pro` / `enterprise` |
| `status` | `varchar(32)` | `trialing` / `active` / `past_due` / `canceled` |

## 2. 迁移规范

- 所有表必须包含 `created_at` 与 `updated_at`。
- 禁止直接修改已发布的迁移文件，必须新增增量迁移脚本。
