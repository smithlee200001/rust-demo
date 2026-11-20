//! 数学工具组：使用有检查的整数运算，使 debug/release 模式具有一致行为。

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    registry::RegistryBuilder,
    tool::{Tool, ToolContext, ToolDescriptor, ToolError, ToolFuture, ToolId, ToolOutput},
};

/// 对两个有符号 64 位整数求和，不接受浮点数或字符串数字。
pub struct AddTool;

/// 必填参数由 Serde 校验类型和整数范围，额外字段被拒绝。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArguments {
    left: i64,
    right: i64,
}

impl Tool for AddTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            // 固定工具名属于源码不变量，启动期检查可及时发现开发错误。
            id: ToolId::new("math.add").expect("Built-in tool ID must be valid"),
            description: "Add two signed 64-bit integers with overflow checking.".to_owned(),
            input_schema: json!({"type": "object", "properties": {"left": {"type": "integer", "minimum": i64::MIN, "maximum": i64::MAX}, "right": {"type": "integer", "minimum": i64::MIN, "maximum": i64::MAX}}, "required": ["left", "right"], "additionalProperties": false}),
        }
    }

    fn execute<'a>(&'a self, context: &'a ToolContext, arguments: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: AddArguments = serde_json::from_value(arguments)
                .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
            // 合法的两个 i64 仍可能产生溢出；返回执行错误而非 panic 或回绕。
            let sum = args.left.checked_add(args.right).ok_or_else(|| {
                ToolError::ExecutionFailed("integer addition overflow".to_owned())
            })?;
            Ok(ToolOutput {
                request_id: context.request_id.clone(),
                data: json!({"sum": sum}),
            })
        })
    }
}

/// 注册数学组工具；不忽略或覆盖重复注册错误。
pub fn register_tools(builder: &mut RegistryBuilder) -> Result<(), ToolError> {
    builder.register(AddTool)
}
