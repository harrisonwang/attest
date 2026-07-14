//! 正文行的扫描工具：strict 模式和机械抽取共用。

use regex::Regex;

/// 逐行返回 (行号, 原文, 遮掉行内代码后的文本)，跳过 fenced 代码块和空行。
/// 遮码是为了让形状匹配只看散文部分——反引号里的 token 归金档管。
pub(crate) fn prose_lines(markdown: &str) -> Vec<(usize, String, String)> {
    let mut fenced = false;
    markdown
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return None;
            }
            if fenced || line.trim().is_empty() {
                return None;
            }
            let mut output = String::with_capacity(line.len());
            let mut inline_code = false;
            for character in line.chars() {
                if character == '`' {
                    inline_code = !inline_code;
                    output.push(' ');
                } else if inline_code {
                    output.push(' ');
                } else {
                    output.push(character);
                }
            }
            Some((index + 1, line.to_owned(), output))
        })
        .collect()
}

/// 散文里"长得像路径"的形状：至少一个斜杠、两段合法字符。
pub(crate) fn path_shape_regex() -> Regex {
    Regex::new(r"(?:^|[\s（(])([A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.@-]+)+/?)")
        .expect("path regex is valid")
}
