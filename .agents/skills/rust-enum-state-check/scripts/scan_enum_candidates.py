#!/usr/bin/env python3
"""Rust 状态字符串初筛脚本。

用于在 Rust 源码中扫描疑似用裸字符串（String / &str）代替 enum 的位置，
输出候选命中供人工或 Agent 结合语义上下文复核。
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

# 跳过的目录：避免扫描构建产物与版本控制目录
IGNORED_DIRS = {"target", ".git", "node_modules", ".idea", ".vscode"}

# 常见表示有限状态或分类的字段关键词（按词根匹配）
STATE_KEYWORDS = (
    "status",
    "state",
    "priority",
    "kind",
    "mode",
    "level",
    "role",
    "phase",
    "stage",
    "category",
    "payment_method",
    "channel",
    "tier",
    "severity",
)

# 明确属于自由文本或标识的后缀，用于减少常见误报
FREE_TEXT_SUFFIXES = (
    "_message",
    "_msg",
    "_text",
    "_desc",
    "_description",
    "_reason",
    "_detail",
    "_details",
    "_url",
    "_path",
    "_id",
    "_name",
    "_label",
)

# 匹配结构体字段定义，例如：`pub status: String,` 或 `priority: Option<&'static str>,`
STRUCT_FIELD_RE = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?P<field>[a-z_][a-z0-9_]*)\s*:\s*(?P<ty>[^,={]+)"
)

# 匹配字符串类型的常见写法
STRING_TYPE_RE = re.compile(
    r"(?:\bString\b|&\s*(?:'[a-z_]+\s+)?str\b|\bCow\s*<\s*(?:'[a-z_]+\s*,\s*)?str\s*>)"
)

# 匹配疑似状态变量与字符串字面量的比较，例如：`status == "paid"` 或 `state.as_str() != "idle"`
STATE_CMP_RE = re.compile(
    r"\b(?P<var>[a-z_][a-z0-9_]*(?:status|state|priority|kind|mode|level|role|phase|stage|category)[a-z0-9_]*)"
    r"(?:\.as_str\(\))?\s*(?:==|!=)\s*\"[^\"]+\""
)

# 匹配对状态变量调用 .as_str() 后进行 match 或 matches!
STATE_MATCH_RE = re.compile(
    r"\b(?:match|matches!\s*\()\s*[^;{}]*\b(?P<var>[a-z_][a-z0-9_]*(?:status|state|priority|kind|mode|level|role|phase|stage|category)[a-z0-9_]*)\.as_str\(\)"
)


@dataclass(frozen=True)
class Finding:
    """表示单条初筛命中记录。"""

    path: Path
    line: int
    category: str
    in_test: bool
    snippet: str


def is_state_field_name(name: str) -> bool:
    """判断字段名是否疑似表示有限状态或分类。"""
    if name.endswith(FREE_TEXT_SUFFIXES):
        return False
    tokens = name.split("_")
    return any(kw in name if "_" in kw else kw in tokens for kw in STATE_KEYWORDS)


def iter_rust_files(root: Path) -> list[Path]:
    """递归收集指定路径下的所有 .rs 文件，自动跳过忽略目录。"""
    if root.is_file():
        return [root] if root.suffix == ".rs" else []

    files: list[Path] = []
    for item in sorted(root.iterdir()):
        if item.name in IGNORED_DIRS:
            continue
        if item.is_dir():
            files.extend(iter_rust_files(item))
        elif item.is_file() and item.suffix == ".rs":
            files.append(item)
    return files


def scan_file(path: Path, base_root: Path) -> list[Finding]:
    """扫描单个 Rust 源码文件，返回初筛命中列表。"""
    findings: list[Finding] = []
    is_test_file = "tests" in path.parts
    in_cfg_test = False
    test_brace_depth = 0

    try:
        content = path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        print(f"[warn] failed to read {path}: {exc}", file=sys.stderr)
        return findings

    rel_path = path.relative_to(base_root) if path.is_relative_to(base_root) else path

    for idx, raw_line in enumerate(content.splitlines(), start=1):
        stripped = raw_line.strip()

        # 跟踪 #[cfg(test)] 模块范围，辅助标注测试上下文
        if stripped.startswith("#[cfg(test)]"):
            in_cfg_test = True
            test_brace_depth = 0
        elif in_cfg_test:
            test_brace_depth += stripped.count("{") - stripped.count("}")
            if test_brace_depth <= 0 and "}" in stripped:
                in_cfg_test = False

        # 跳过纯注释行
        if stripped.startswith("//") or stripped.startswith("/*") or stripped.startswith("*"):
            continue

        in_test_ctx = is_test_file or in_cfg_test

        field_match = STRUCT_FIELD_RE.match(raw_line)
        if field_match:
            field_name = field_match.group("field")
            field_type = field_match.group("ty").strip()
            if is_state_field_name(field_name) and STRING_TYPE_RE.search(field_type):
                findings.append(
                    Finding(
                        path=rel_path,
                        line=idx,
                        category="struct-string-field",
                        in_test=in_test_ctx,
                        snippet=stripped,
                    )
                )

        if STATE_CMP_RE.search(raw_line) or STATE_MATCH_RE.search(raw_line):
            findings.append(
                Finding(
                    path=rel_path,
                    line=idx,
                    category="string-state-branch",
                    in_test=in_test_ctx,
                    snippet=stripped,
                )
            )

    return findings


def main() -> int:
    """脚本入口：解析参数、执行扫描并输出英文报告。"""
    parser = argparse.ArgumentParser(
        description="Scan Rust source files for state fields or branches using raw strings."
    )
    parser.add_argument(
        "target",
        nargs="?",
        default=".",
        help="Target file or directory to scan (default: current directory).",
    )
    parser.add_argument(
        "--fail-on-findings",
        action="store_true",
        help="Exit with non-zero status (1) when any candidate is found (useful for CI).",
    )
    args = parser.parse_args()

    target = Path(args.target).resolve()
    if not target.exists():
        print(f"[error] target path does not exist: {target}", file=sys.stderr)
        return 2

    base_root = target if target.is_dir() else target.parent
    rust_files = iter_rust_files(target)
    all_findings: list[Finding] = []
    for rust_file in rust_files:
        all_findings.extend(scan_file(rust_file, base_root))

    print(f"[info] scanned {len(rust_files)} Rust file(s) under {target}")
    if not all_findings:
        print("[ok] no suspicious string-based state candidates found")
        return 0

    print(f"[found] {len(all_findings)} candidate(s) requiring semantic review:")
    for item in all_findings:
        ctx_tag = " [in-test]" if item.in_test else ""
        print(f"  {item.path}:{item.line} [{item.category}]{ctx_tag} {item.snippet}")

    return 1 if args.fail_on_findings else 0


if __name__ == "__main__":
    sys.exit(main())
