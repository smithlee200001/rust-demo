//! 按功能分组注册；增加工具时在对应模块实现接口，并加入该组的显式注册清单。

pub mod catalog;
pub mod math;
pub mod text;

use std::{collections::BTreeMap, sync::Arc};

use crate::{
    registry::{RegistryBuilder, ToolRegistry},
    tool::ToolError,
};

/// 组装全部内置工具；任一注册失败立即返回错误，不发布部分初始化的注册中心。
pub fn register_all_tools() -> Result<ToolRegistry, ToolError> {
    let mut builder = RegistryBuilder::default();
    text::register_tools(&mut builder)?;
    math::register_tools(&mut builder)?;
    // 这里是明确的演示数据，真实应用可在启动阶段加载配置后注入同一工具。
    let entries = BTreeMap::from([
        (
            "rust".to_owned(),
            "A systems programming language.".to_owned(),
        ),
        (
            "tokio".to_owned(),
            "An asynchronous runtime for Rust.".to_owned(),
        ),
    ]);
    catalog::register_tools(&mut builder, Arc::new(entries))?;
    Ok(builder.build())
}
