# serde-demo

演示如何使用 [`serde`](https://serde.rs/) 和 [`serde_json`](https://docs.rs/serde_json/) 将 Rust `enum` 转换为字符串，以及从字符串反序列化回枚举。

## 示例内容

代码位于 [src/main.rs](./src/main.rs)，包含以下场景：

1. **默认变体名称序列化**：使用 `serde_json::to_string` 将 `OrderStatus` 序列化为带双引号的 JSON 字符串（如 `"Shipped"`）。
2. **提取不带引号的纯字符串**：通过 `enum_to_plain_string` 借助 `serde_json::to_value` 提取 `Value::String`，得到纯文本字符串（如 `Shipped`），并对带负载的枚举返回 `EnumStringError::NotAString`。
3. **自定义命名规则**：使用 `#[serde(rename_all = "snake_case")]` 和 `#[serde(rename = "p0_urgent")]` 控制 `TaskPriority` 序列化后的字符串格式，以及 `#[serde(rename_all = "kebab-case")]` 控制 `PaymentMethod` 的输出格式。
4. **纯字符串反序列化与错误处理**：通过 `enum_from_plain_string` 将字符串解析回枚举，并展示非法枚举字符串返回错误的处理方式。
5. **结构体组合多个枚举**：通过 `OrderRecord` 演示在同一个结构体中同时使用 `OrderStatus`、`TaskPriority` 和 `PaymentMethod` 三个不同命名策略的 `enum` 字段，完成 JSON 序列化与反序列化。

## 依赖

- `serde = { version = "1.0", features = ["derive"] }`
- `serde_json = "1.0"`

## 运行方式

在仓库根目录执行：

```bash
cd serde-demo
cargo run
```

运行单元测试：

```bash
cd serde-demo
cargo test
```

## 限制说明

- `enum_to_plain_string` 仅适用于序列化结果为 JSON 字符串的无负载单元枚举（unit enum）；包含字段的枚举变体默认会序列化为 JSON 对象，因此会返回错误。
- 本示例重点演示基于 `serde` 的字符串转换，未引入 `strum` 等专门处理枚举字符串的第三方 crate。
