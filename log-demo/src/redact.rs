//! 日志写入前的主目录用户名脱敏，不查询实际文件系统，也不依赖当前操作系统。
//!
//! 这是路径模式过滤器，不是通用隐私检测器：不处理 URL 编码、Unicode 转义、
//! 自定义主目录、含字段分隔标点的非常规用户名、用户名在消息其他位置的重复出现，
//! 或任意敏感字段。调用方应先
//! 对结构化字符串逐项脱敏，再序列化；不要把已编码的 JSON 当作完整安全边界。

/// 将绝对主目录路径中的用户名替换为 `***`，保留根目录、分隔符和后续文件路径。
///
/// 支持 `/home/name`、`/Users/name`、Windows 盘符路径以及双反斜杠转义形式。
/// Windows 的 `Users`/`home` 大小写不敏感；Unix 路径保持大小写敏感。
/// 用户名可以含 Unicode 和空格。无引号的文本存在歧义时，宁可多遮盖后续文字，
/// 直到路径分隔符或明确的字段边界，也不把空格后的姓名留在日志中。
/// 不解析任意嵌套转义：JSON 字符串应由调用方解码后逐项传入。
/// 函数只分配返回字符串，不执行 I/O，不改变输入；重复调用保持相同结果。
pub fn redact(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut copied = 0;
    let mut cursor = 0;
    while cursor < input.len() {
        if let Some(start) = username_start(input, cursor) {
            let quote = input[..cursor]
                .chars()
                .next_back()
                .filter(|c| matches!(c, '\'' | '"'));
            let mut end = start;
            for (offset, ch) in input[start..].char_indices() {
                let at = start + offset;
                // 后续另一条绝对路径不能被第一个用户名吞掉，否则会漏掉第二个用户名。
                if (at > start && username_start(input, at).is_some())
                    || ch == '/'
                    || ch == '\\'
                    || ch.is_control()
                    || matches!(
                        ch,
                        '"' | ',' | ';' | '=' | '|' | '[' | ']' | '{' | '}' | '(' | ')' | '<' | '>'
                    )
                    || quote == Some(ch)
                {
                    break;
                }
                end = at + ch.len_utf8();
            }
            // 空字段、导航段及已经脱敏的段不应再次处理。尾部空白属于外层消息。
            let name = input[start..end].trim_end();
            if !name.is_empty() && !matches!(name, "." | ".." | "***") {
                let end = start + name.len();
                output.push_str(&input[copied..start]);
                output.push_str("***");
                copied = end;
                cursor = end;
                continue;
            }
        }
        // cursor 总在 UTF-8 字符边界；按字符前进，避免非 ASCII 用户名切片越界。
        cursor += input[cursor..].chars().next().map_or(1, char::len_utf8);
    }
    output.push_str(&input[copied..]);
    output
}

/// 在合法的绝对路径起点识别主目录，返回用户名的字节偏移；不接受嵌套 `/srv/home/`。
fn username_start(input: &str, at: usize) -> Option<usize> {
    let previous = input[..at].chars().next_back();
    if previous.is_some_and(|ch| {
        ch.is_alphanumeric() || matches!(ch, '/' | '\\' | '_' | '-' | '.' | ':' | '@' | '%' | '~')
    }) {
        return None;
    }
    let bytes = input.as_bytes();
    let mut cursor = at;
    let drive = bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(cursor + 1) == Some(&b':');
    if drive {
        cursor += 2;
    }
    let separator = *bytes.get(cursor)?;
    if !matches!(separator, b'/' | b'\\') {
        return None;
    }
    cursor += 1;
    // Debug/JSON 中的反斜杠通常成对出现。保留原始表示，不在这里整体解码。
    if separator == b'\\' {
        while bytes.get(cursor) == Some(&b'\\') {
            cursor += 1;
        }
    }
    let root_start = cursor;
    while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
        cursor += 1;
    }
    let root = &input[root_start..cursor];
    let windows = drive || separator == b'\\';
    let supported = if windows {
        root.eq_ignore_ascii_case("Users") || root.eq_ignore_ascii_case("home")
    } else {
        matches!(root, "Users" | "home")
    };
    if !supported || !bytes.get(cursor).is_some_and(|b| matches!(b, b'/' | b'\\')) {
        return None;
    }
    let next_separator = bytes[cursor];
    cursor += 1;
    if next_separator == b'\\' {
        while bytes.get(cursor) == Some(&b'\\') {
            cursor += 1;
        }
    }
    Some(cursor)
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn unix_paths_and_terminal_usernames() {
        for (input, expected) in [
            ("/home/alice/project", "/home/***/project"),
            ("/Users/bob", "/Users/***"),
            ("path=/home/张三/报告.txt", "path=/home/***/报告.txt"),
            ("'/Users/Alice Smith/Documents'", "'/Users/***/Documents'"),
            ("\"/home/Alice Smith\"", "\"/home/***\""),
            ("/Users/Alice Smith", "/Users/***"),
            ("/home/O'Connor/data", "/home/***/data"),
        ] {
            assert_eq!(redact(input), expected, "input: {input}");
        }
    }

    #[test]
    fn windows_drive_and_escaped_separators() {
        for (input, expected) in [
            (r"C:\Users\Alice\data", r"C:\Users\***\data"),
            (r"d:/users/Alice Smith/data", r"d:/users/***/data"),
            (r"C:\\Users\\Alice Smith\\data", r"C:\\Users\\***\\data"),
            (
                r#"{"path":"C:\\Users\\Alice"}"#,
                r#"{"path":"C:\\Users\\***"}"#,
            ),
            (r"C:\UsErS\张三", r"C:\UsErS\***"),
            (r"\Users\Alice\data", r"\Users\***\data"),
            (r"C:/Users/Alice\data", r"C:/Users/***\data"),
        ] {
            assert_eq!(redact(input), expected, "input: {input}");
        }
    }

    #[test]
    fn unrelated_and_incomplete_paths_are_unchanged() {
        for input in [
            "",
            "/home/",
            "/home",
            "/Users",
            "/srv/home/alice",
            "some/home/alice",
            "https://example.com/home/alice",
            "/homework/alice",
            "C:/Usernames/alice",
            "/home/../data",
            "/home/./data",
            "alice",
            "/HOME/alice",
            "/home//data",
            r"\\server\Users\alice",
        ] {
            assert_eq!(redact(input), input, "input: {input}");
        }
    }

    #[test]
    fn multiple_paths_delimiters_and_repeated_redaction() {
        for (input, expected) in [
            (
                "from=/home/alice/a to=C:/Users/bob/b",
                "from=/home/***/a to=C:/Users/***/b",
            ),
            ("/home/alice; /Users/bob", "/home/***; /Users/***"),
            ("/home/alice\n/Users/bob", "/home/***\n/Users/***"),
            ("/home/alice C:/Users/bob", "/home/*** C:/Users/***"),
            ("[/home/alice], (/Users/bob)", "[/home/***], (/Users/***)"),
        ] {
            let redacted = redact(input);
            assert_eq!(redacted, expected);
            assert_eq!(redact(&redacted), redacted);
        }
    }

    #[test]
    fn ambiguous_spaces_favor_privacy_over_message_preservation() {
        assert_eq!(
            redact("failed: /home/Alice Smith could not open"),
            "failed: /home/***"
        );
    }
}
