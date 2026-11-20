//! 定义工具标识、元数据、执行上下文和支持动态分发的异步接口。

use std::{error::Error, fmt, future::Future, pin::Pin};

use serde_json::Value;

/// 经过校验的工具标识，例如 `text.echo`；内部字符串不能被外部修改。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolId(String);

impl ToolId {
    /// 接受不超过 64 字节的点分名称，每段以小写字母开头，后接小写字母、数字或下划线。
    /// 空名称、空分段和非法字符均返回 `InvalidId`，不自动修正用户输入。
    pub fn new(name: &str) -> Result<Self, ToolError> {
        let valid = !name.is_empty()
            && name.len() <= 64
            && name.split('.').all(|part| {
                part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            });
        if !valid {
            return Err(ToolError::InvalidId(name.to_owned()));
        }
        Ok(Self(name.to_owned()))
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// 工具的发现信息；参数描述用于展示，实际校验由具体工具完成。
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub description: String,
    pub input_schema: Value,
}

/// 单次调用的信息；长期共享依赖应通过具体工具的构造函数注入。
#[derive(Debug)]
pub struct ToolContext {
    /// 用于关联调用和结果；本示例不实现自动日志、取消或超时。
    pub request_id: String,
}

/// 统一返回关联请求标识和结构化业务数据。
#[derive(Debug, serde::Serialize)]
pub struct ToolOutput {
    pub request_id: String,
    pub data: Value,
}

/// 使用枚举区分配置、查找、参数和执行错误，调用方无需匹配错误文本。
#[derive(Debug, PartialEq, Eq)]
pub enum ToolError {
    InvalidId(String),
    DuplicateTool(ToolId),
    ToolNotFound(ToolId),
    InvalidArguments(String),
    ExecutionFailed(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(id) => write!(f, "Invalid tool ID: {id:?}"),
            Self::DuplicateTool(id) => write!(f, "Tool already registered: {id}"),
            Self::ToolNotFound(id) => write!(f, "Tool not found: {id}"),
            Self::InvalidArguments(message) => write!(f, "Invalid arguments: {message}"),
            Self::ExecutionFailed(message) => write!(f, "Tool execution failed: {message}"),
        }
    }
}

impl Error for ToolError {}

/// 装箱 Future 使异步接口可用于 `dyn Tool`；生命周期限制其不能超过工具和上下文。
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// 所有工具实现的协议；实例可被共享和并发调用，实现者需同步内部可变状态。
pub trait Tool: Send + Sync {
    /// 返回元数据；注册中心只在注册时读取并保存快照。
    fn descriptor(&self) -> ToolDescriptor;

    /// 执行一次调用：参数为 JSON，返回统一结果或参数/执行错误。
    /// 实现应避免长时间阻塞异步线程；本示例仅执行有长度上限的内存运算。
    fn execute<'a>(&'a self, context: &'a ToolContext, arguments: Value) -> ToolFuture<'a>;
}
