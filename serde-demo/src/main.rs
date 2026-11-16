//! `serde-demo` 示例：演示如何使用 `serde` 将 Rust `enum` 转换为字符串，以及从字符串反序列化回枚举。
//!
//! 涵盖常见场景：
//! 1. 默认枚举变体名称序列化（JSON 字符串与纯字符串提取）
//! 2. 使用 `#[serde(rename_all = ...)]` 和 `#[serde(rename = ...)]` 自定义字符串格式
//! 3. 结合 `Result` 将枚举安全地转换为纯 `String`（不带 JSON 双引号）
//! 4. 字符串反序列化回枚举及未知变体的错误处理

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::json;
use std::error::Error;
use std::fmt;

/// 订单状态枚举：演示默认的变体名序列化与反序列化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum OrderStatus {
    /// 待支付
    Pending,
    /// 已支付
    Paid,
    /// 已发货
    Shipped,
    /// 已完成
    Completed,
    /// 已取消
    Cancelled,
}

/// 日志级别枚举：演示通过 `rename_all` 统一转换为小写蛇形命名，以及单个变体覆盖命名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TaskPriority {
    /// 低优先级 -> "low"
    Low,
    /// 普通优先级 -> "normal"
    Normal,
    /// 高优先级 -> "high"
    High,
    /// 紧急优先级，使用 `rename` 单独指定为 "p0_urgent"
    #[serde(rename = "p0_urgent")]
    Urgent,
}

/// 支付方式枚举：演示通过 `kebab-case` 转换为短横线分隔的小写字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PaymentMethod {
    /// 信用卡 -> "credit-card"
    CreditCard,
    /// 银行转账 -> "bank-transfer"
    BankTransfer,
    /// 支付宝 -> "alipay"
    Alipay,
}

/// 订单记录结构体：演示在同一个结构体中组合多个不同命名策略的 `enum` 字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OrderRecord {
    /// 订单编号
    id: String,
    /// 订单状态（默认 PascalCase 变体名）
    status: OrderStatus,
    /// 处理优先级（`snake_case` 与自定义 `rename`）
    priority: TaskPriority,
    /// 支付方式（`kebab-case`）
    payment_method: PaymentMethod,
}

/// 枚举转纯字符串时可能出现的错误类型。
#[derive(Debug, PartialEq, Eq)]
enum EnumStringError {
    /// `serde_json` 序列化失败
    Serialize(String),
    /// 序列化结果不是字符串（例如带负载的复杂枚举默认会序列化为 JSON 对象）
    NotAString(String),
}

impl fmt::Display for EnumStringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(msg) => write!(f, "failed to serialize enum: {msg}"),
            Self::NotAString(actual) => {
                write!(f, "expected enum to serialize to a string, got: {actual}")
            }
        }
    }
}

impl Error for EnumStringError {}

/// 将支持 `Serialize` 的无负载枚举转换为不带引号的纯 `String`。
///
/// 说明：
/// - `serde_json::to_string(&value)` 会保留 JSON 字符串的双引号（如 `"Pending"`）。
/// - 通过 `serde_json::to_value` 先转为 `serde_json::Value`，再提取 `Value::String`，
///   即可得到纯文本字符串（如 `Pending`）。
///
/// 错误条件：
/// - 当 `T` 无法被序列化，或序列化结果不是 `Value::String`（例如包含字段的枚举变体）时返回 `Err`。
fn enum_to_plain_string<T: Serialize>(value: &T) -> Result<String, EnumStringError> {
    let json_value =
        serde_json::to_value(value).map_err(|err| EnumStringError::Serialize(err.to_string()))?;

    match json_value {
        Value::String(text) => Ok(text),
        other => Err(EnumStringError::NotAString(other.to_string())),
    }
}

/// 从纯字符串（不带 JSON 双引号）反序列化为目标枚举类型。
///
/// 参数：
/// - `raw`: 待解析的枚举名称字符串，需与 `serde` 配置的命名规则一致。
///
/// 返回值：
/// - 匹配成功时返回对应的枚举实例；否则返回 `serde_json::Error`。
fn enum_from_plain_string<T>(raw: &str) -> Result<T, serde_json::Error>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(Value::String(raw.to_owned()))
}

fn main() -> Result<(), Box<dyn Error>> {
    // 1. 使用 serde_json::to_string 将枚举转换为 JSON 字符串（带双引号）
    let status = OrderStatus::Shipped;
    let json_str = serde_json::to_string(&status)?;
    println!("[json string] OrderStatus::Shipped -> {json_str}");

    // 2. 使用辅助函数转换为不带双引号的纯字符串
    let plain_str = enum_to_plain_string(&status)?;
    println!("[plain string] OrderStatus::Shipped -> {plain_str}");

    // 3. 演示 rename_all 与 rename 的转换效果
    let priorities = [
        TaskPriority::Low,
        TaskPriority::Normal,
        TaskPriority::High,
        TaskPriority::Urgent,
    ];
    for priority in priorities {
        let name = enum_to_plain_string(&priority)?;
        println!("[renamed] {priority:?} -> {name}");
    }

    // 4. 演示从纯字符串反序列化回枚举
    let parsed: TaskPriority = enum_from_plain_string("p0_urgent")?;
    println!("[deserialized] \"p0_urgent\" -> {parsed:?}");

    // 5. 演示非法字符串反序列化时的错误处理
    match enum_from_plain_string::<OrderStatus>("UnknownStatus") {
        Ok(val) => println!("[unexpected] parsed: {val:?}"),
        Err(err) => println!("[expected error] invalid enum string rejected: {err}"),
    }

    // 6. 演示包含多个 enum 字段的结构体序列化与反序列化
    let order = OrderRecord {
        id: "ORD-20261116-001".to_owned(),
        status: OrderStatus::Paid,
        priority: TaskPriority::Urgent,
        payment_method: PaymentMethod::CreditCard,
    };
    let order_json = serde_json::to_string(&order)?;
    println!("[struct json] OrderRecord -> {order_json}");

    let decoded_order: OrderRecord = serde_json::from_str(&order_json)?;
    println!("[struct deserialized] -> {decoded_order:?}");

    // 7. 使用 json! 宏直接构造包含枚举值的 JSON 对象（对象键必须使用字符串字面量）
    println!(
        "[struct plain string] OrderRecord -> {}",
        json!({
            "priority": TaskPriority::Urgent,
            "payment_method": PaymentMethod::CreditCard,
            "id": "ORD-20261116-001",
            "status": OrderStatus::Paid,
        })
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证带字段的复杂枚举：用于测试 `enum_to_plain_string` 对非纯字符串枚举的拒绝逻辑。
    #[derive(Serialize)]
    enum PayloadEnum {
        WithData(u32),
    }

    #[test]
    fn default_enum_serializes_to_json_and_plain_string() {
        let status = OrderStatus::Completed;
        let json_str = serde_json::to_string(&status).expect("OrderStatus should serialize");
        assert_eq!(json_str, "\"Completed\"");

        let plain_str =
            enum_to_plain_string(&status).expect("unit enum should convert to plain string");
        assert_eq!(plain_str, "Completed");
    }

    #[test]
    fn rename_all_and_field_rename_work_as_expected() {
        assert_eq!(
            enum_to_plain_string(&TaskPriority::Normal).expect("Normal should serialize"),
            "normal"
        );
        assert_eq!(
            enum_to_plain_string(&TaskPriority::Urgent).expect("Urgent should serialize"),
            "p0_urgent"
        );
    }

    #[test]
    fn round_trip_from_plain_string_succeeds() {
        let original = OrderStatus::Paid;
        let encoded = enum_to_plain_string(&original).expect("OrderStatus should serialize");
        let decoded: OrderStatus =
            enum_from_plain_string(&encoded).expect("OrderStatus should deserialize");
        assert_eq!(decoded, original);
    }

    #[test]
    fn invalid_plain_string_returns_error() {
        let result = enum_from_plain_string::<TaskPriority>("invalid_priority");
        assert!(result.is_err());
    }

    #[test]
    fn payload_enum_is_rejected_by_plain_string_helper() {
        let err = enum_to_plain_string(&PayloadEnum::WithData(42))
            .expect_err("payload enum should not serialize to a plain string");
        assert!(matches!(err, EnumStringError::NotAString(_)));
    }

    #[test]
    fn struct_with_multiple_enums_round_trip() {
        let order = OrderRecord {
            id: "ORD-1001".to_owned(),
            status: OrderStatus::Paid,
            priority: TaskPriority::Urgent,
            payment_method: PaymentMethod::CreditCard,
        };

        let json_str = serde_json::to_string(&order).expect("OrderRecord should serialize");
        assert_eq!(
            json_str,
            r#"{"id":"ORD-1001","status":"Paid","priority":"p0_urgent","payment_method":"credit-card"}"#
        );

        let decoded: OrderRecord =
            serde_json::from_str(&json_str).expect("OrderRecord should deserialize");
        assert_eq!(decoded, order);
    }

    #[test]
    fn struct_with_invalid_enum_field_returns_error() {
        let invalid_json =
            r#"{"id":"ORD-1002","status":"Paid","priority":"normal","payment_method":"cash"}"#;
        let result = serde_json::from_str::<OrderRecord>(invalid_json);
        assert!(result.is_err());
    }
}
