use std::path::Path;

use crate::{
    Namespace, RepoFacts, Tier, Token, TokenSource,
    extract::{has_dynamic_placeholder, has_wildcard},
};

use super::Resolution;

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    let raw_candidate = match &token.command {
        Some(command) if command.program.contains('/') => command.program.as_str(),
        Some(_) => return Resolution::NoMatch,
        None => token.text.trim().trim_matches('`'),
    };
    let (candidate, source_locator) = split_source_locator(raw_candidate);
    if has_dynamic_placeholder(candidate) || candidate.starts_with('@') {
        return Resolution::Ignored;
    }
    if pure_parent_traversal(candidate) {
        return Resolution::Ignored;
    }
    if has_wildcard(candidate) {
        if source_locator.is_some() || !looks_like_glob_path(candidate) {
            return Resolution::NoMatch;
        }
        let mut matches = facts
            .path_bases(&token.doc)
            .into_iter()
            .flat_map(|base| facts.glob_paths(&token.doc, base, candidate))
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        let Some(referent) = matches.first().cloned() else {
            return Resolution::Ignored;
        };
        return Resolution::Bound {
            ns: Namespace::Path,
            referent,
            tier: Tier::Normalized,
            alternatives: matches.into_iter().skip(1).collect(),
        };
    }
    // 链接目标在语法上就是文件引用，不用再靠长相判断它是不是路径；
    // 反引号 token 什么都可能是，得先过形状门。
    if token.source != TokenSource::LinkTarget && !looks_like_path(candidate) {
        return Resolution::NoMatch;
    }
    let candidate = candidate.trim_end_matches(['.', ',', ':', ';']);
    let bases = facts.path_bases(&token.doc);
    let mut matches = Vec::new();
    for base in bases {
        if let Some(path) = facts.resolve_path(&token.doc, base, candidate) {
            if !matches.contains(&path) {
                matches.push(path);
            }
        }
    }
    if let Some(referent) = matches.first().cloned() {
        if let Some(locator) = source_locator
            && !source_locator_is_bound(locator, &referent, facts)
        {
            return Resolution::NearMiss {
                ns: Namespace::Path,
                suggestion: None,
                note: "文件存在，但行号或符号定位对不上，可能已经过时".to_owned(),
                searched: vec!["source locator".into()],
                alternatives: vec![referent],
            };
        }
        let tier = if candidate.starts_with("./") || candidate.starts_with("../") {
            Tier::Normalized
        } else {
            Tier::Exact
        };
        return Resolution::Bound {
            ns: Namespace::Path,
            referent,
            tier,
            alternatives: matches.into_iter().skip(1).collect(),
        };
    }

    let mut contextual_matches = Vec::new();
    for expanded in contextual_candidates(token, candidate) {
        for base in facts.path_bases(&token.doc) {
            if let Some(path) = facts.resolve_path(&token.doc, base, &expanded)
                && !contextual_matches.contains(&path)
            {
                contextual_matches.push(path);
            }
        }
    }
    if let Some(referent) = contextual_matches.first().cloned() {
        if let Some(locator) = source_locator
            && !source_locator_is_bound(locator, &referent, facts)
        {
            return Resolution::NearMiss {
                ns: Namespace::Path,
                suggestion: None,
                note: "文件存在，但行号或符号定位对不上，可能已经过时".to_owned(),
                searched: vec!["contextual parent path".into()],
                alternatives: vec![referent],
            };
        }
        return Resolution::Bound {
            ns: Namespace::Path,
            referent,
            tier: Tier::Normalized,
            alternatives: contextual_matches.into_iter().skip(1).collect(),
        };
    }

    if Path::new(candidate).extension().is_none() {
        for extension in ["ts", "tsx", "js", "jsx", "rs", "py", "go"] {
            let expanded = format!("{candidate}.{extension}");
            for base in facts.path_bases(&token.doc) {
                if let Some(referent) = facts.resolve_path(&token.doc, base, &expanded) {
                    return Resolution::Bound {
                        ns: Namespace::Path,
                        referent,
                        tier: Tier::Normalized,
                        alternatives: Vec::new(),
                    };
                }
            }
        }
    }

    if facts.path_ignored(candidate) {
        return Resolution::NearMiss {
            ns: Namespace::Path,
            suggestion: None,
            note: "路径命中仓库的 ignore 规则，多半是运行时产物".to_owned(),
            searched: vec![".gitignore".into()],
            alternatives: Vec::new(),
        };
    }
    // owner/repo 这类仓库缩写不做"搬家"猜测，不然会被仓库里的同名目录误配上。
    // 缩写的典型形状：正好两段、末段无扩展名、不带尾斜杠，且首段在仓库里不存在。
    // 真实的文件搬家要么带扩展名，要么以 / 结尾，要么首段还在，不受影响。
    let segments: Vec<&str> = candidate.split('/').collect();
    let slug_shape = segments.len() == 2
        && !candidate.ends_with('/')
        && Path::new(candidate).extension().is_none()
        && !facts
            .path_bases(&token.doc)
            .into_iter()
            .any(|base| facts.file_exists(&token.doc, base, segments[0]));
    if !slug_shape {
        let basename = Path::new(candidate)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(candidate);
        let relocated = facts.find_basename(basename);
        if !relocated.is_empty() {
            // 唯一命中才敢说"应改为"；多个同名就只列候选，留给人判断。
            let suggestion = (relocated.len() == 1).then(|| relocated[0].clone());
            return Resolution::NearMiss {
                ns: Namespace::Path,
                suggestion,
                note: "同名文件出现在仓库其他位置，路径可能已移动".to_owned(),
                searched: vec!["doc-dir".into(), "project-root".into(), "repo-root".into()],
                alternatives: relocated,
            };
        }
    }
    if facts.has_go_mod() && looks_like_go_import(candidate) {
        return Resolution::NoMatch;
    }
    let parent = Path::new(candidate)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    let parentless = parent.is_empty() || parent == ".";
    if source_locator.is_none()
        && !parentless
        && !facts
            .path_bases(&token.doc)
            .into_iter()
            .any(|base| facts.file_exists(&token.doc, base, parent))
    {
        return Resolution::NoMatch;
    }
    // SKILL.md 教 agent 去目标仓库干活，里面的根级裸文件名多半说的是
    // "用户仓库里该有的文件"或"跑完才生成的产物"，在本仓库里毫无踪迹
    // 不足以定罪，降成提醒。带目录的路径不受影响——skill 自带的
    // references/ 附件缺了照样红。
    if parentless && source_locator.is_none() && token.doc.ends_with("SKILL.md") {
        return Resolution::NearMiss {
            ns: Namespace::Path,
            suggestion: None,
            note: "SKILL.md 常在描述目标仓库或运行时产物，根级文件名在本仓库无踪迹时只提醒"
                .to_owned(),
            searched: vec!["doc-dir".into(), "project-root".into(), "repo-root".into()],
            alternatives: Vec::new(),
        };
    }
    Resolution::Broken {
        ns: Namespace::Path,
        searched: vec!["doc-dir".into(), "project-root".into(), "repo-root".into()],
        suggestion: None,
    }
}

fn pure_parent_traversal(value: &str) -> bool {
    let components = value
        .trim_end_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    !components.is_empty()
        && components.contains(&"..")
        && components
            .iter()
            .all(|component| matches!(*component, "." | ".."))
}

fn contextual_candidates(token: &Token, candidate: &str) -> Vec<String> {
    let leaf = candidate.trim_end_matches('/');
    if leaf.is_empty() || leaf.contains('/') {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for (index, span) in token.context.split('`').enumerate() {
        if index % 2 == 0 {
            continue;
        }
        let raw = span.trim().trim_end_matches(['.', ',', ':', ';']);
        if raw == candidate
            || raw == leaf
            || has_dynamic_placeholder(raw)
            || has_wildcard(raw)
            || raw.contains(char::is_whitespace)
        {
            continue;
        }
        let repo_relative = raw.trim_start_matches('/').trim_end_matches('/');
        let directory_shape = raw.ends_with('/')
            || (raw.starts_with('/') && Path::new(repo_relative).extension().is_none());
        if !directory_shape
            || repo_relative.is_empty()
            || !repo_relative.split('/').all(valid_path_component)
        {
            continue;
        }
        let expanded = format!("{repo_relative}/{leaf}");
        if !candidates.contains(&expanded) {
            candidates.push(expanded);
        }
    }
    candidates
}

fn looks_like_glob_path(value: &str) -> bool {
    !value.contains("://")
        && !value.starts_with(['~', '/'])
        && !value.contains(char::is_whitespace)
        && (value.contains('/') || Path::new(value).extension().is_some())
}

fn source_locator_is_bound(locator: &str, referent: &str, facts: &dyn RepoFacts) -> bool {
    let symbol = locator.strip_suffix("()").unwrap_or(locator);
    !symbol.is_empty()
        && symbol
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
        && facts
            .grep_word(symbol)
            .is_some_and(|hit| hit.path == referent)
}

fn split_source_locator(value: &str) -> (&str, Option<&str>) {
    let Some((path, locator)) = value.rsplit_once(':') else {
        return (value, None);
    };
    let path_has_extension = Path::new(path).extension().is_some();
    let locator_shape = !locator.is_empty()
        && locator.len() <= 128
        && locator
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-().".contains(character));
    if path_has_extension && locator_shape {
        (path, Some(locator))
    } else {
        (value, None)
    }
}

fn looks_like_path(value: &str) -> bool {
    if value.contains("://")
        || value.starts_with('~')
        || value.starts_with('/')
        || value.contains(" / ")
        || value.contains(char::is_whitespace)
    {
        return false;
    }
    let known_extension = Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            if extension != extension.to_ascii_lowercase() {
                return false;
            }
            matches!(
                extension,
                "md" | "rs"
                    | "js"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "py"
                    | "go"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "json"
                    | "sh"
                    | "zsh"
                    | "html"
                    | "css"
                    | "sql"
                    | "lock"
                    | "txt"
                    | "env"
                    | "mod"
                    | "sum"
                    | "work"
            )
        });
    value.starts_with("./")
        || value.starts_with("../")
        || value.ends_with('/')
        || known_extension
        || (value.contains('/') && value.split('/').all(valid_path_component))
}

fn valid_path_component(component: &str) -> bool {
    !component.is_empty()
        && component
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-@".contains(character))
}

fn looks_like_go_import(value: &str) -> bool {
    !value.starts_with('.')
        && !value.ends_with('/')
        && value.contains('/')
        && (value.starts_with("github.com/")
            || value.starts_with("golang.org/")
            || value.starts_with("gopkg.in/")
            || value
                .split('/')
                .next()
                .is_some_and(|head| !head.contains('.')))
}
