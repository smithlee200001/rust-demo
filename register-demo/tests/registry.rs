//! 从调用方视角验证注册、查找、执行及错误边界，不依赖注册中心内部结构。

use register_demo::{
    registry::RegistryBuilder,
    tool::{ToolContext, ToolError, ToolId},
    tools::{
        ids::{CATALOG_LOOKUP_ID, MATH_ADD_ID, MATH_DIVIDE_ID, TEXT_ECHO_ID, TEXT_STATS_ID},
        register_all_tools,
        text::EchoTool,
    },
};
use serde_json::json;

/// 每次调用使用独立上下文，测试请求标识能够穿过分发层返回。
fn context() -> ToolContext {
    ToolContext {
        request_id: "test-request".to_owned(),
    }
}

/// 标识校验覆盖空分段、非 ASCII 字符、长度和大小写约束。
#[test]
fn validates_tool_ids() {
    for name in [
        "",
        "text..echo",
        ".echo",
        "echo.",
        "Text.echo",
        "text.echo-1",
        "text.回显",
        "1tool",
    ] {
        assert!(matches!(ToolId::new(name), Err(ToolError::InvalidId(_))));
    }
    assert!(ToolId::new(&"a".repeat(64)).is_ok());
    assert!(ToolId::new(&"a".repeat(65)).is_err());
    assert!(ToolId::new("text.echo_1").is_ok());
}

/// 查询顺序稳定且构建空注册中心有效；非法名称与不存在的工具错误不同。
#[test]
fn lists_and_looks_up_tools() -> Result<(), ToolError> {
    assert!(RegistryBuilder::default().build().list().is_empty());
    let registry = register_all_tools()?;
    let names: Vec<_> = registry
        .list()
        .iter()
        .map(|item| item.id.to_string())
        .collect();
    // 保留协议名称的字面值断言，防止修改常量时无意改变外部调用契约。
    assert_eq!(
        names,
        [
            "catalog.lookup",
            "math.add",
            "math.divide",
            "text.echo",
            "text.stats"
        ]
    );
    assert!(registry.get(TEXT_ECHO_ID).is_ok());
    assert!(matches!(
        registry.get("missing.tool"),
        Err(ToolError::ToolNotFound(_))
    ));
    assert!(matches!(registry.get(""), Err(ToolError::InvalidId(_))));
    Ok(())
}

/// 重复注册失败后原实例仍然可调用，不出现丢失注册项的副作用。
#[tokio::test]
async fn duplicate_registration_preserves_original() -> Result<(), ToolError> {
    let mut builder = RegistryBuilder::default();
    builder.register(EchoTool)?;
    assert!(matches!(
        builder.register(EchoTool),
        Err(ToolError::DuplicateTool(_))
    ));
    let registry = builder.build();
    assert_eq!(registry.list().len(), 1);
    let output = registry
        .execute(TEXT_ECHO_ID, &context(), json!({"text": "still available"}))
        .await?;
    assert_eq!(output.data, json!({"text": "still available"}));
    Ok(())
}

/// 真实执行验证名称分发、返回数据和关联 ID，并检查未知工具不会被调用。
#[tokio::test]
async fn dispatches_tools() -> Result<(), ToolError> {
    let registry = register_all_tools()?;
    let output = registry
        .execute(MATH_ADD_ID, &context(), json!({"left": -2, "right": 44}))
        .await?;
    assert_eq!(output.data, json!({"sum": 42}));
    assert_eq!(output.request_id, "test-request");
    assert!(matches!(
        registry
            .execute("missing.tool", &context(), json!({}))
            .await,
        Err(ToolError::ToolNotFound(_))
    ));
    Ok(())
}

/// 空文本有效，长度限制按 UTF-8 字节计算；类型错误和未知字段必须明确失败。
#[tokio::test]
async fn validates_text_arguments() -> Result<(), ToolError> {
    let registry = register_all_tools()?;
    for text in [String::new(), "a".repeat(4096), "你".repeat(1365)] {
        let output = registry
            .execute(TEXT_ECHO_ID, &context(), json!({"text": text}))
            .await?;
        assert_eq!(output.data, json!({"text": text}));
    }
    for input in [
        json!({}),
        json!(null),
        json!({"text": 1}),
        json!({"text": "ok", "extra": true}),
        json!({"text": "a".repeat(4097)}),
        json!({"text": "你".repeat(1366)}),
    ] {
        assert!(matches!(
            registry.execute(TEXT_ECHO_ID, &context(), input).await,
            Err(ToolError::InvalidArguments(_))
        ));
    }
    Ok(())
}

/// 整数边界可以正常相加，但上下溢出以及不符合参数类型的 JSON 必须返回错误。
#[tokio::test]
async fn validates_math_arguments_and_overflow() -> Result<(), ToolError> {
    let registry = register_all_tools()?;
    for value in [i64::MIN, i64::MAX] {
        let output = registry
            .execute(MATH_ADD_ID, &context(), json!({"left": value, "right": 0}))
            .await?;
        assert_eq!(output.data, json!({"sum": value}));
    }
    for (left, right) in [(i64::MAX, 1), (i64::MIN, -1)] {
        assert!(matches!(
            registry
                .execute(
                    MATH_ADD_ID,
                    &context(),
                    json!({"left": left, "right": right})
                )
                .await,
            Err(ToolError::ExecutionFailed(_))
        ));
    }
    for input in [
        json!({"left": 1}),
        json!({"left": "1", "right": 2}),
        json!({"left": 1.5, "right": 2}),
        json!({"left": u64::MAX, "right": 0}),
        json!({"left": 1, "right": 2, "extra": 3}),
    ] {
        assert!(matches!(
            registry.execute(MATH_ADD_ID, &context(), input).await,
            Err(ToolError::InvalidArguments(_))
        ));
    }
    Ok(())
}

/// 获取的共享实例独立持有生命周期；注册中心销毁后仍可完成调用。
#[tokio::test]
async fn tool_handle_outlives_registry() -> Result<(), ToolError> {
    let registry = register_all_tools()?;
    let tool = registry.get(TEXT_ECHO_ID)?;
    drop(registry);
    let output = tool.execute(&context(), json!({"text": "alive"})).await?;
    assert_eq!(output.data, json!({"text": "alive"}));
    Ok(())
}

/// 验证 Unicode 标量值与字节的差异；组合音标算两个标量值，空白用于分词。
#[tokio::test]
async fn counts_text_and_validates_limits() -> Result<(), ToolError> {
    let registry = register_all_tools()?;
    for (text, expected) in [
        ("", json!({"bytes": 0, "chars": 0, "words": 0})),
        ("你 好\n", json!({"bytes": 8, "chars": 4, "words": 2})),
        ("e\u{301}", json!({"bytes": 3, "chars": 2, "words": 1})),
        ("a\u{3000}b", json!({"bytes": 5, "chars": 3, "words": 2})),
    ] {
        let output = registry
            .execute(TEXT_STATS_ID, &context(), json!({"text": text}))
            .await?;
        assert_eq!(output.data, expected);
        assert_eq!(output.request_id, "test-request");
    }
    let output = registry
        .execute(TEXT_STATS_ID, &context(), json!({"text": "a".repeat(4096)}))
        .await?;
    assert_eq!(
        output.data,
        json!({"bytes": 4096, "chars": 4096, "words": 1})
    );
    for input in [
        json!({}),
        json!({"text": false}),
        json!({"text": "ok", "extra": 1}),
        json!({"text": "你".repeat(1366)}),
    ] {
        assert!(matches!(
            registry.execute(TEXT_STATS_ID, &context(), input).await,
            Err(ToolError::InvalidArguments(_))
        ));
    }
    Ok(())
}

/// 验证正负整数的截断规则、合法极值，以及除零和 MIN / -1 的不同错误类型。
#[tokio::test]
async fn divides_integers_safely() -> Result<(), ToolError> {
    let registry = register_all_tools()?;
    for (dividend, divisor, quotient, remainder) in [
        (17, 5, 3, 2),
        (-17, 5, -3, -2),
        (17, -5, -3, 2),
        (0, 1, 0, 0),
        (i64::MIN, 1, i64::MIN, 0),
    ] {
        let output = registry
            .execute(
                MATH_DIVIDE_ID,
                &context(),
                json!({"dividend": dividend, "divisor": divisor}),
            )
            .await?;
        assert_eq!(
            output.data,
            json!({"quotient": quotient, "remainder": remainder})
        );
        assert_eq!(output.request_id, "test-request");
    }
    for input in [
        json!({"dividend": 1, "divisor": 0}),
        json!({"dividend": 1}),
        json!({"dividend": "1", "divisor": 2}),
        json!({"dividend": 1, "divisor": 2, "extra": 0}),
        json!({"dividend": u64::MAX, "divisor": 1}),
    ] {
        assert!(matches!(
            registry.execute(MATH_DIVIDE_ID, &context(), input).await,
            Err(ToolError::InvalidArguments(_))
        ));
    }
    assert!(matches!(
        registry
            .execute(
                MATH_DIVIDE_ID,
                &context(),
                json!({"dividend": i64::MIN, "divisor": -1})
            )
            .await,
        Err(ToolError::ExecutionFailed(_))
    ));
    Ok(())
}

/// 注入自定义数据验证工具没有写死默认目录，同时区分空值、未命中和大小写。
#[tokio::test]
async fn looks_up_injected_catalog() -> Result<(), ToolError> {
    use register_demo::tools::catalog;
    use std::{collections::BTreeMap, sync::Arc};

    let entries = Arc::new(BTreeMap::from([
        ("custom".to_owned(), "injected value".to_owned()),
        ("empty".to_owned(), String::new()),
        ("a".repeat(64), "boundary".to_owned()),
    ]));
    let mut builder = RegistryBuilder::default();
    catalog::register_tools(&mut builder, Arc::clone(&entries))?;
    drop(entries);
    let registry = builder.build();
    for (key, value) in [
        ("custom", Some("injected value")),
        ("empty", Some("")),
        ("Custom", None),
        ("missing", None),
    ] {
        let output = registry
            .execute(CATALOG_LOOKUP_ID, &context(), json!({"key": key}))
            .await?;
        assert_eq!(
            output.data,
            json!({"key": key, "found": value.is_some(), "value": value})
        );
        assert_eq!(output.request_id, "test-request");
    }
    let output = registry
        .execute(
            CATALOG_LOOKUP_ID,
            &context(),
            json!({"key": "a".repeat(64)}),
        )
        .await?;
    assert_eq!(output.data["value"], "boundary");
    for input in [
        json!({}),
        json!({"key": ""}),
        json!({"key": 1}),
        json!({"key": "custom", "extra": true}),
        json!({"key": "a".repeat(65)}),
        json!({"key": "你".repeat(22)}),
    ] {
        assert!(matches!(
            registry.execute(CATALOG_LOOKUP_ID, &context(), input).await,
            Err(ToolError::InvalidArguments(_))
        ));
    }
    Ok(())
}
