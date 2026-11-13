# 本地开发环境搭建（Local Setup）

## 1. 依赖准备

- Node.js >= 22 / Docker Desktop / PostgreSQL 16（或使用 `docker compose up -d`）

## 2. 启动步骤

```bash
cp .env.example .env
docker compose up -d postgres redis
npm install
npm run db:migrate
npm run dev
```
