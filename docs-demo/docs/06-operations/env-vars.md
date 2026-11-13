# 环境变量清单（Environment Variables）

| 变量名 | 必填 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- |
| `NODE_ENV` | 是 | `development` | 运行环境：`development` / `staging` / `production` |
| `DATABASE_URL` | 是 | - | PostgreSQL 连接串 |
| `REDIS_URL` | 是 | - | Redis 连接串 |
| `JWT_PRIVATE_KEY` | 是 | - | RS256 私钥（生产环境从密钥管理服务注入） |
| `LOG_LEVEL` | 否 | `info` | 日志级别：`debug` / `info` / `warn` / `error` |
