use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};

use crate::{CommandToken, Token, TokenSource};

pub fn extract_tokens(doc: &str, markdown: &str) -> Vec<Token> {
    let line_starts = line_starts(markdown);
    let mut tokens = Vec::new();
    let mut shell_fence = false;
    let mut heredoc_delimiter = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(language))) => {
                shell_fence = matches!(
                    language.trim().to_ascii_lowercase().as_str(),
                    "bash" | "sh" | "shell" | "zsh" | "console"
                );
                heredoc_delimiter = None;
            }
            Event::Start(Tag::Link(_, destination, _) | Tag::Image(_, destination, _)) => {
                let Some(target) = link_destination(&destination) else {
                    continue;
                };
                let (line, column_start, context) = location(markdown, &line_starts, range.start);
                tokens.push(Token {
                    column_end: column_start + target.chars().count(),
                    text: target,
                    doc: doc.to_owned(),
                    line,
                    column_start,
                    context,
                    source: TokenSource::LinkTarget,
                    command: None,
                });
            }
            Event::End(Tag::CodeBlock(_)) => {
                shell_fence = false;
                heredoc_delimiter = None;
            }
            Event::Code(text) => {
                let raw = text.trim();
                if raw.is_empty() {
                    continue;
                }
                let (line, column_start, context) = location(markdown, &line_starts, range.start);
                tokens.push(Token {
                    text: raw.to_owned(),
                    doc: doc.to_owned(),
                    line,
                    column_start,
                    column_end: column_start + raw.chars().count(),
                    context,
                    source: TokenSource::InlineCode,
                    command: inline_command(raw),
                });
            }
            Event::Text(text) if shell_fence => {
                for (raw_line, relative_offset) in logical_shell_lines(&text) {
                    let trimmed = raw_line.trim();
                    if let Some(delimiter) = heredoc_delimiter.as_deref() {
                        if trimmed.trim_start_matches('\t') == delimiter {
                            heredoc_delimiter = None;
                        }
                        continue;
                    }
                    if let Some(delimiter) = parse_heredoc_delimiter(trimmed) {
                        heredoc_delimiter = Some(delimiter);
                        continue;
                    }
                    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.contains("<<") {
                        continue;
                    }
                    if let Some(command) = parse_command_line(trimmed) {
                        let (line, column_start, context) =
                            location(markdown, &line_starts, range.start + relative_offset);
                        tokens.push(Token {
                            text: trimmed.trim_start_matches("$ ").to_owned(),
                            doc: doc.to_owned(),
                            line,
                            column_start,
                            column_end: column_start + trimmed.chars().count(),
                            context,
                            source: TokenSource::ShellFence,
                            command: Some(command),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    tokens
}

/// 从链接目标里筛出本仓库的文件引用。带 scheme 的是外部世界，`#` 开头的是
/// 页内跳转，`/` 和 `~` 开头的不在仓库相对语义里——这些都不是本仓库的断言。
/// 剩下的去掉锚点和查询串，才是能拿去文件树对质的路径。
fn link_destination(destination: &str) -> Option<String> {
    let destination = destination.trim();
    if destination.is_empty() || destination.starts_with('#') || destination.starts_with("//") {
        return None;
    }
    if let Some((scheme, _)) = destination.split_once(':')
        && !scheme.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    {
        return None;
    }
    let path = destination
        .split(['#', '?'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches("./");
    if path.is_empty() || path.starts_with('/') || path.starts_with('~') {
        return None;
    }
    Some(path.to_owned())
}

fn inline_command(raw: &str) -> Option<CommandToken> {
    let first = raw.split_whitespace().next()?;
    let command_shape = raw.starts_with("$ ")
        || (raw.contains(char::is_whitespace)
            && !first.contains('/')
            && !first.contains('=')
            && !raw.contains(" -> "));
    command_shape.then(|| parse_command_line(raw)).flatten()
}

pub fn parse_command_line(input: &str) -> Option<CommandToken> {
    let input = input.trim().trim_start_matches("$ ").trim();
    if input.is_empty() || input.contains("<<") {
        return None;
    }
    let words = shell_words(input);
    let mut index = 0;
    while index < words.len() && is_env_assignment(&words[index]) {
        index += 1;
    }
    let program = words.get(index)?.to_owned();
    if has_dynamic_placeholder(&program) || matches!(program.as_str(), "cd" | "export" | "unset") {
        return None;
    }
    Some(CommandToken {
        program,
        args: words[index + 1..].to_vec(),
    })
}

fn shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '#' if current.is_empty() => break,
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn is_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(crate) fn has_dynamic_placeholder(value: &str) -> bool {
    value.contains("...")
        || value.contains('<')
        || value.contains('>')
        || value.contains('{')
        || value.contains('}')
        || value.contains('[')
        || value.contains(']')
        || value.contains('$')
        || value.contains('…')
        || value.contains('%')
}

pub(crate) fn has_wildcard(value: &str) -> bool {
    value.contains('*') || value.contains('?')
}

fn logical_shell_lines(text: &str) -> Vec<(String, usize)> {
    let mut lines = Vec::new();
    let mut logical = String::new();
    let mut logical_offset = 0;
    let mut byte_offset = 0;
    for segment in text.split_inclusive('\n') {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if logical.is_empty() {
            logical_offset = byte_offset;
        }
        let trimmed = line.trim_end();
        if let Some(prefix) = trimmed.strip_suffix('\\') {
            logical.push_str(prefix);
            logical.push(' ');
        } else {
            logical.push_str(trimmed);
            lines.push((std::mem::take(&mut logical), logical_offset));
        }
        byte_offset += segment.len();
    }
    if !logical.is_empty() {
        lines.push((logical, logical_offset));
    }
    lines
}

fn parse_heredoc_delimiter(line: &str) -> Option<String> {
    let marker = line.find("<<")?;
    let mut tail = line[marker + 2..].trim_start();
    if let Some(stripped) = tail.strip_prefix('-') {
        tail = stripped.trim_start();
    }
    if tail.starts_with('<') {
        return None;
    }
    let delimiter = tail
        .split(|character: char| character.is_whitespace() || ";|&".contains(character))
        .next()?
        .trim_matches(['\'', '"']);
    (!delimiter.is_empty()).then(|| delimiter.to_owned())
}

fn line_starts(markdown: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(index, _)| index + 1))
        .collect()
}

fn location(markdown: &str, starts: &[usize], offset: usize) -> (usize, usize, String) {
    let line_index = starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1);
    let line_start = starts[line_index];
    let line_end = markdown[line_start..]
        .find('\n')
        .map(|relative| line_start + relative)
        .unwrap_or(markdown.len());
    let context_start = starts[context_start_line(markdown, starts, line_index)];
    (
        line_index + 1,
        markdown[line_start..offset].chars().count() + 1,
        markdown[context_start..line_end].to_owned(),
    )
}

fn context_start_line(markdown: &str, starts: &[usize], line_index: usize) -> usize {
    let fallback = line_index.saturating_sub(2);
    let lower_bound = line_index.saturating_sub(12);
    let mut label = None;
    for index in (lower_bound..line_index).rev() {
        let line_start = starts[index];
        let line_end = starts
            .get(index + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(markdown.len());
        let line = markdown[line_start..line_end].trim();
        if line.starts_with('#') {
            return index;
        }
        if label.is_none()
            && ((line.starts_with("**") && line.contains(':'))
                || line.ends_with(':')
                || line.contains("github.com/")
                || line.contains("gitlab.com/"))
        {
            label = Some(index);
        }
    }
    label.unwrap_or(fallback).min(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_inline_and_only_labeled_shell_fences() {
        let markdown = "Run `cargo test`.\n\n```bash\nFOO=bar pnpm run test # now\n```\n\n```\nnot a command\n```\n";
        let tokens = extract_tokens("AGENTS.md", markdown);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].command.as_ref().unwrap().program, "cargo");
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[1].command.as_ref().unwrap().program, "pnpm");
        assert_eq!(tokens[1].command.as_ref().unwrap().args, ["run", "test"]);
    }

    #[test]
    fn skips_heredocs_and_placeholders() {
        assert!(parse_command_line("cat <<EOF").is_none());
        assert!(parse_command_line("<command> --flag").is_none());
    }

    #[test]
    fn skips_complete_heredocs_and_preserves_continuation_locations() {
        let markdown =
            "```bash\ncat <<'EOF'\nnpm run missing\nEOF\npnpm run \\\n  test\ngit status\n```\n";
        let tokens = extract_tokens("AGENTS.md", markdown);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].command.as_ref().unwrap().args, ["run", "test"]);
        assert_eq!(tokens[0].line, 5);
        assert_eq!(tokens[1].text, "git status");
        assert_eq!(tokens[1].line, 7);
    }

    #[test]
    fn link_targets_keep_local_paths_and_drop_the_external_world() {
        let markdown = "先读 [架构](docs/arch.md#设计)，配图在 ![图](./assets/a.png)。\n\n外部的 [官网](https://example.com)、[邮件](mailto:a@b.c)、页内的 [跳转](#安装) 和绝对路径 [根](/etc/hosts) 都不算。\n";
        let tokens = extract_tokens("AGENTS.md", markdown);

        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>(),
            ["docs/arch.md", "assets/a.png"]
        );
        assert!(
            tokens
                .iter()
                .all(|token| token.source == TokenSource::LinkTarget)
        );
        assert_eq!(tokens[0].line, 1);
    }

    #[test]
    fn carries_section_introductions_into_token_context() {
        let markdown = "## Reference\n\n[upstream](https://github.com/example/project) is an external reference.\n\n- `one/`\n- `two/`\n- `three/`\n- `four/`\n- `five/`\n- `six/`\n- `seven/`\n";
        let tokens = extract_tokens("AGENTS.md", markdown);
        let last = tokens.last().unwrap();

        assert!(last.context.contains("## Reference"));
        assert!(last.context.contains("github.com/example/project"));
    }
}
