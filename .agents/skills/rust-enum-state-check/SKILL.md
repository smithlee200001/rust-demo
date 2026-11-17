---
name: rust-enum-state-check
description: >-
  Check Rust code for state, priority, category, or mode fields represented as raw
  strings (String/&str) instead of strongly typed enums with serde mapping.
  Use during code review, refactoring, or auditing Rust projects.
---

# Rust Enum State Check

用于检测 Rust 代码中本应使用 `enum` 建模却误用裸字符串（`String` / `&str`）表示状态、类型、优先级或模式的代码，并给出基于 `enum` + `serde` 的重构建议。

## 检查流程

### 第一步：运行初筛脚本

在目标仓库或指定子目录执行初筛脚本（仅依赖 Python 3 标准库）：

```bash
python3 .agents/skills/rust-enum-state-check/scripts/scan_enum_candidates.py .
```

也可以只扫描单个案例目录，例如：

```bash
python3 .agents/skills/rust-enum-state-check/scripts/scan_enum_candidates.py serde-demo
```

脚本会输出两类候选命中（按 `file:line [category] snippet` 格式）：
- `struct-string-field`：结构体中疑似表示状态/分类但类型为字符串（如 `String`、`&str`、`Option<String>`）的字段。
- `string-state-branch`：使用字符串字面量进行状态比较或 `match` 分支的表达式。

### 第二步：语义复核（排除误报）

初筛结果存在合理豁免或误报可能，必须结合上下文逐条阅读命中行前后代码进行复核：

1. **确认是否为有限集合**：
   - **违规**：取值是固定有限集合（如订单状态、任务优先级、支付方式、运行模式、日志级别）。
   - **豁免**：自由文本、唯一标识或外部透传文案（例如 `status_message`、`error_detail`、`id`、`raw_payload`）。
2. **区分业务代码与测试/示例反例**：
   - 脚本标注 `[in-test]` 的命中通常位于 `#[cfg(test)]` 或 `tests/` 中，需确认是否为测试输入构造（例如构造非法 JSON 字符串测试反序列化报错）或文档示例反例。
3. **检查边界序列化策略**：
   - 如果已有 `enum` 负责外部协议交互，确认其是否显式标注了 `#[serde(rename_all = "...")]` 或 `#[serde(rename = "...")]`，以及是否需要 `#[serde(other)]` 兼容未知扩展值。

### 第三步：输出审查结论与重构建议

按以下结构向用户汇总结果：

1. **扫描范围与统计**：扫描了多少个 `.rs` 文件、初筛命中数、复核后确认违规数。
2. **确认需要改造的位置**（如有）：列出文件路径、行号、当前写法与建议改造的 `enum` + `serde` 定义（默认使用 `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]`）。
3. **误报/豁免说明**（如有）：简要说明哪些命中属于自由文本、测试构造或协议透传，无需修改。
