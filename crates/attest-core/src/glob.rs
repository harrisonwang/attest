//! 两种通配匹配，语义刻意分开：
//!
//! - [`glob_match`]：按 `/` 分段的路径 glob，`*` 与 `?` 不跨段，`**` 匹配任意层目录。
//!   文档发现、scope 配置、路径 token 的通配绑定都用它，CLI 和测试替身共用一份实现，
//!   避免测试用一套语义、生产用另一套。
//! - [`name_match`]：不分段的名字通配，给脚本名、包名这类允许含 `/` 的名字空间用
//!   （比如 `pnpm --filter @app/*`）。

/// 路径 glob。pattern 与 path 都按仓库相对形式传入（不带前导 `/`）。
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let path_segments: Vec<&str> = path.split('/').collect();
    // 按段做可达性推进：`**` 吃零到多段，其余段内用名字通配。
    let mut reachable = vec![false; path_segments.len() + 1];
    reachable[0] = true;
    for pattern_segment in &pattern_segments {
        let mut next = vec![false; path_segments.len() + 1];
        if *pattern_segment == "**" {
            let mut any = false;
            for (index, slot) in next.iter_mut().enumerate() {
                any = any || reachable[index];
                *slot = any;
            }
        } else {
            for index in 0..path_segments.len() {
                if reachable[index] && name_match(pattern_segment, path_segments[index]) {
                    next[index + 1] = true;
                }
            }
        }
        reachable = next;
    }
    reachable[path_segments.len()]
}

/// 名字通配：`*` 任意串、`?` 任意单字符，对 `/` 不做特殊处理。
pub(crate) fn name_match(pattern: &str, candidate: &str) -> bool {
    let mut previous = vec![false; candidate.chars().count() + 1];
    previous[0] = true;
    let candidate: Vec<_> = candidate.chars().collect();
    for pattern_char in pattern.chars() {
        let mut current = vec![false; candidate.len() + 1];
        if pattern_char == '*' {
            current[0] = previous[0];
        }
        for (index, candidate_char) in candidate.iter().enumerate() {
            current[index + 1] = match pattern_char {
                '*' => previous[index + 1] || current[index],
                '?' => previous[index],
                _ => previous[index] && pattern_char == *candidate_char,
            };
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_star_matches_root_and_nested_paths() {
        assert!(glob_match("**/AGENTS.md", "AGENTS.md"));
        assert!(glob_match("**/AGENTS.md", "apps/api/AGENTS.md"));
        assert!(glob_match(".claude/**/*.md", ".claude/skills/attest/SKILL.md"));
        assert!(glob_match("docs/**", "docs/design/notes.md"));
        assert!(!glob_match("docs/**/*.md", "src/main.rs"));
    }

    #[test]
    fn single_star_stays_inside_one_segment() {
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("src/*.rs", "src/nested/main.rs"));
        assert!(glob_match("src/*/mod.rs", "src/auth/mod.rs"));
    }

    #[test]
    fn name_match_crosses_slashes_for_package_names() {
        assert!(name_match("@app/*", "@app/api"));
        assert!(name_match("test:*", "test:unit"));
        assert!(!name_match("test:*", "lint"));
    }
}
