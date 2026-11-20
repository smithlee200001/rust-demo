//! 文本工具组：展示 JSON 到强类型参数的转换与输入边界校验。

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    registry::RegistryBuilder,
    tool::{Tool, ToolContext, ToolDescriptor, ToolError, ToolFuture, ToolId, ToolOutput},
};

/// 回显工具；无可变状态，因此同一实例可以安全地并发调用。
pub struct EchoTool;

/// 拒绝未知字段以便及时发现调用方拼写错误；空字符串是合法输入。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EchoArguments {
    text: String,
}

impl Tool for EchoTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            // 固定标识由开发者维护；失败说明源码常量错误，应在启动时终止。
            id: ToolId::new("text.echo").expect("Built-in tool ID must be valid"),
            description: "Return the supplied text (up to 4096 UTF-8 bytes).".to_owned(),
            input_schema: json!({"type": "object", "properties": {"text": {"type": "string", "description": "At most 4096 UTF-8 bytes"}}, "required": ["text"], "additionalProperties": false}),
        }
    }

    fn execute<'a>(&'a self, context: &'a ToolContext, arguments: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: EchoArguments = serde_json::from_value(arguments)
                .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
            // 按 UTF-8 字节限制输入，避免将字符数和内存占用混淆；不会截断字符。
            if args.text.len() > 4096 {
                return Err(ToolError::InvalidArguments(
                    "text exceeds 4096 UTF-8 bytes".to_owned(),
                ));
            }
            Ok(ToolOutput {
                request_id: context.request_id.clone(),
                data: json!({"text": args.text}),
            })
        })
    }
}

/// 注册文本组工具；将名称冲突传播给启动流程处理。
pub fn register_tools(builder: &mut RegistryBuilder) -> Result<(), ToolError> {
    builder.register(EchoTool)
}
