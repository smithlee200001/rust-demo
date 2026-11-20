//! 目录工具组：通过构造函数注入只读共享数据，不依赖全局变量或真实数据库。

use std::{collections::BTreeMap, sync::Arc};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    registry::RegistryBuilder,
    tool::{Tool, ToolContext, ToolDescriptor, ToolError, ToolFuture, ToolId, ToolOutput},
};

/// 按键精确查询的工具；Arc 保持目录存活，只读映射无需互斥锁。
pub struct LookupTool {
    entries: Arc<BTreeMap<String, String>>,
}

impl LookupTool {
    /// 注入键值目录，允许空目录；键按原文区分大小写，不进行裁剪或归一化。
    /// 可查询的键需为 1–64 个 UTF-8 字节；较长键即使存在也不能通过执行接口查询。
    pub fn new(entries: Arc<BTreeMap<String, String>>) -> Self {
        Self { entries }
    }
}

/// 查询仅接受一个必填字符串键；未知字段作为参数错误返回。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupArguments {
    key: String,
}

impl Tool for LookupTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            id: ToolId::new("catalog.lookup").expect("Built-in tool ID must be valid"),
            description: "Look up an exact, case-sensitive key in the injected in-memory catalog."
                .to_owned(),
            input_schema: json!({"type": "object", "properties": {"key": {"type": "string", "minLength": 1, "description": "At most 64 UTF-8 bytes; exact case-sensitive match"}}, "required": ["key"], "additionalProperties": false}),
        }
    }

    fn execute<'a>(&'a self, context: &'a ToolContext, arguments: Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: LookupArguments = serde_json::from_value(arguments)
                .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
            if args.key.is_empty() || args.key.len() > 64 {
                return Err(ToolError::InvalidArguments(
                    "key must contain 1 to 64 UTF-8 bytes".to_owned(),
                ));
            }
            // 未命中是正常查询结果；用 found 区分未命中与目录中存储的空字符串。
            let value = self.entries.get(&args.key);
            Ok(ToolOutput {
                request_id: context.request_id.clone(),
                data: json!({"key": args.key, "found": value.is_some(), "value": value}),
            })
        })
    }
}

/// 注册目录组并转移共享句柄；重复名称错误传播给启动流程。
pub fn register_tools(
    builder: &mut RegistryBuilder,
    entries: Arc<BTreeMap<String, String>>,
) -> Result<(), ToolError> {
    builder.register(LookupTool::new(entries))
}
