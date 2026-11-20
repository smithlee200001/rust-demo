//! 将启动阶段的可变注册与运行阶段的只读调用分开，执行时不需要注册表锁。

use std::{collections::HashMap, sync::Arc};

use serde_json::Value;

use crate::tool::{Tool, ToolContext, ToolDescriptor, ToolError, ToolId, ToolOutput};

/// 同时保存共享实例和注册时的描述快照，避免后续描述变化影响索引。
struct Entry {
    tool: Arc<dyn Tool>,
    descriptor: ToolDescriptor,
}

/// 启动阶段使用的注册器；重复注册不会替换已经存在的工具。
#[derive(Default)]
pub struct RegistryBuilder {
    tools: HashMap<ToolId, Entry>,
}

impl RegistryBuilder {
    /// 接收具体工具并取得所有权；名称冲突时返回 `DuplicateTool`，保留原工具。
    pub fn register(&mut self, tool: impl Tool + 'static) -> Result<(), ToolError> {
        let descriptor = tool.descriptor();
        match self.tools.entry(descriptor.id.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => {
                Err(ToolError::DuplicateTool(descriptor.id))
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Entry {
                    tool: Arc::new(tool),
                    descriptor,
                });
                Ok(())
            }
        }
    }

    /// 消耗注册器，生成不可修改的注册中心；允许构建空注册中心。
    pub fn build(self) -> ToolRegistry {
        ToolRegistry { tools: self.tools }
    }
}

/// 运行阶段的只读注册中心；可通过 `Arc<ToolRegistry>` 在任务间共享。
pub struct ToolRegistry {
    tools: HashMap<ToolId, Entry>,
}

impl ToolRegistry {
    /// 校验名称并查找工具；返回的 Arc 独立持有实例生命周期，不暴露注册表修改权。
    pub fn get(&self, name: &str) -> Result<Arc<dyn Tool>, ToolError> {
        let id = ToolId::new(name)?;
        self.tools
            .get(&id)
            .map(|entry| Arc::clone(&entry.tool))
            .ok_or(ToolError::ToolNotFound(id))
    }

    /// 按名称升序返回描述快照，避免 HashMap 遍历顺序导致展示结果不稳定。
    pub fn list(&self) -> Vec<&ToolDescriptor> {
        let mut descriptors: Vec<_> = self.tools.values().map(|entry| &entry.descriptor).collect();
        descriptors.sort_by(|left, right| left.id.cmp(&right.id));
        descriptors
    }

    /// 查找并异步执行工具；原样传播参数和执行错误，不自动重试有副作用的操作。
    pub async fn execute(
        &self,
        name: &str,
        context: &ToolContext,
        arguments: Value,
    ) -> Result<ToolOutput, ToolError> {
        self.get(name)?.execute(context, arguments).await
    }
}
