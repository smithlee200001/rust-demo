//! 按功能分组注册；增加工具时在对应模块实现接口，并加入该组的显式注册清单。

pub mod math;
pub mod text;

use crate::{
    registry::{RegistryBuilder, ToolRegistry},
    tool::ToolError,
};

/// 组装全部内置工具；任一注册失败立即返回错误，不发布部分初始化的注册中心。
pub fn register_all_tools() -> Result<ToolRegistry, ToolError> {
    let mut builder = RegistryBuilder::default();
    text::register_tools(&mut builder)?;
    math::register_tools(&mut builder)?;
    Ok(builder.build())
}
