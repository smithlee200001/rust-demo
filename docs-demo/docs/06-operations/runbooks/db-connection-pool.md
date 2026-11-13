# Runbook：数据库连接池耗尽排查

## 告警现象

应用日志大量出现 `TimeoutError: ResourceRequest timed out` 或接口 P99 延迟突增。

## 快速排查步骤

1. 检查数据库活跃连接数与长事务：
   ```sql
   SELECT state, count(*) FROM pg_stat_activity GROUP BY state;
   ```
2. 排查是否存在未归还的连接（特别是事务中嵌套了外部 HTTP 请求）。
3. 临时止血：重启异常 Pod 或适当调大连接池上限（需同步评估 DB 最大连接数）。
