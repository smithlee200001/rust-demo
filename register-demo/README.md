# 工具注册与统一管理

独立的 Rust Edition 2024 Cargo 工程，展示多个工具如何通过统一接口注册、查询和异步执行。依赖 Serde/serde_json 处理强类型 JSON 参数，Tokio 驱动异步入口和测试。

## 设计

```text
启动阶段：register_all_tools → 各组 register_tools → RegistryBuilder → build
运行阶段：调用方 → ToolRegistry.execute → Arc<dyn Tool> → 具体工具
```

- `Tool`：统一 `descriptor()` 和 `execute(context, arguments)`。异步执行返回装箱的 `Future`，支持 trait object 动态分发。
- `ToolId`：经过校验的名称，长度为 1–64 个 ASCII 字节，各点分段以小写字母开头，后续允许小写字母、数字、下划线。
- `ToolDescriptor`：名称、说明和 JSON 参数描述，注册时保存快照；`list()` 按名称排序。
- `RegistryBuilder`：显式注册具体实例；重复名称报错，保留原实例。`build()` 消耗构建器。
- `ToolRegistry`：构建后只读，提供 `get`、`list`、`execute`，不需要注册表读写锁。空注册中心也有效。
- `ToolContext` / `ToolOutput`：传递并返回请求 ID，结果数据统一使用 JSON。
- `ToolError`：枚举区分非法名称、重复注册、工具不存在、参数错误和执行失败。

工具通过 `Arc<dyn Tool>` 共享，`Tool: Send + Sync` 约束实现的线程安全。`get()` 返回的句柄在注册中心销毁后仍可使用，最后一个引用释放时销毁实例。新增有状态工具时，长期依赖通过构造函数注入，内部可变状态由工具自行同步。

## 文件结构

```text
src/
├── lib.rs             # 对外模块入口
├── main.rs            # 默认演示及命令行调用
├── tool.rs            # 接口、标识、元数据、上下文与错误
├── registry.rs        # 注册构建器和只读注册中心
└── tools/
    ├── mod.rs         # 集中组装所有分组
    ├── text.rs        # 文本工具组
    └── math.rs        # 数学工具组
tests/registry.rs      # 调用方视角的行为与边界测试
```

## 内置工具

| 名称 | 参数 | 结果 | 边界 |
| --- | --- | --- | --- |
| `text.echo` | `{"text":"hello"}` | `{"text":"hello"}` | 允许空文本，最多 4096 个 UTF-8 字节 |
| `math.add` | `{"left":20,"right":22}` | `{"sum":42}` | 两个 i64，使用 checked_add 检查溢出 |

两个工具都是真实的内存运算，不使用模拟网络响应。参数字段必填，拒绝未知字段及类型不符的输入。`input_schema` 供调用方发现和展示；注册中心没有通用 JSON Schema 验证器，运行时由具体工具反序列化并检查业务边界。文本长度按字节限制，通过字段描述说明，未使用按字符计数的 `maxLength` 冒充字节限制。

## 运行

从仓库根目录执行：

```bash
cd register-demo
cargo run
cargo run -- text.echo '{"text":"Hello, registry!"}'
cargo run -- math.add '{"left":20,"right":22}'
```

无参数时按名称列出工具，并依次执行回显和求和。指定工具时，输出包含 `request_id` 和 `data` 的 JSON。示例请求 ID 固定为 `demo-request`。

以下命令在 `register-demo` 目录执行，用于观察错误，预期返回非零退出码：

```bash
cargo run -- missing.tool '{}'
cargo run -- text.echo '{"text":123}'
cargo run -- math.add '{"left":9223372036854775807,"right":1}'
```

## 增加工具

1. 在 `src/tools/` 的相应功能模块定义结构体，实现 `Tool`。
2. `descriptor()` 提供稳定唯一的名称及与实际行为一致的参数描述。
3. `execute()` 将 JSON 转成独立参数结构体，校验边界后返回 `ToolOutput`；错误使用 `ToolError`，消息使用英文。
4. 在所属模块的 `register_tools()` 中注册；如果增加分组，在 `register_all_tools()` 中调用新分组。
5. 补充可观察行为和失败边界测试。

## 验证

在 `register-demo` 目录执行：

```bash
cargo fmt
cargo fmt --check
cargo check
cargo clippy --all-targets -- -D warnings
cargo test
```

测试覆盖名称校验、空注册中心、稳定列举、重复注册、未知工具、调用分发、请求 ID、UTF-8 字节限制、参数错误、整数溢出和共享句柄生命周期。

## 限制

本例采用显式分组注册，不会自动扫描所有 trait 实现，不支持运行时增删或动态插件。异步接口便于以后加入 I/O 工具，但当前工具执行的是短小内存运算。没有实现 HTTP/文件访问、超时、取消、权限控制、重试或并发限流；共享 Arc 本身不限制并发。JSON 在进入工具前已解析成内存对象，文本长度校验不等同于入口请求体大小限制。
