//! 可运行入口：默认展示全部工具，也可通过命令行按名称传入 JSON 参数。

use std::{env, error::Error, io};

use register_tools_demo::{
    tool::ToolContext,
    tools::{
        ids::{CATALOG_LOOKUP_ID, MATH_ADD_ID, MATH_DIVIDE_ID, TEXT_ECHO_ID, TEXT_STATS_ID},
        register_all_tools,
    },
};
use serde_json::json;

/// 支持无参数演示或 `<tool-name> '<json>'`；失败交由 main 返回非零退出码。
/// 使用单线程运行时演示异步协议，本程序不创建后台任务或外部连接。
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let registry = register_all_tools()?;
    let context = ToolContext {
        request_id: "demo-request".to_owned(),
    };
    let arguments: Vec<_> = env::args().skip(1).collect();

    match arguments.as_slice() {
        [] => {
            for descriptor in registry.list() {
                println!(
                    "Registered tool: {} - {}",
                    descriptor.id, descriptor.description
                );
            }
            for (name, input) in [
                (TEXT_ECHO_ID, json!({"text": "Hello, tools!"})),
                (MATH_ADD_ID, json!({"left": 20, "right": 22})),
                (TEXT_STATS_ID, json!({"text": "Hello 世界"})),
                (MATH_DIVIDE_ID, json!({"dividend": 17, "divisor": 5})),
                (CATALOG_LOOKUP_ID, json!({"key": "rust"})),
            ] {
                let output = registry.execute(name, &context, input).await?;
                println!("Result for {name}: {}", serde_json::to_string(&output)?);
            }
        }
        [name, input] => {
            let output = registry
                .execute(name, &context, serde_json::from_str(input)?)
                .await?;
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Usage: register-tools-demo [<tool-name> '<json-arguments>']",
            )
            .into());
        }
    }
    Ok(())
}
