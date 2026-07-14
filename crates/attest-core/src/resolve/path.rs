use std::path::Path;

use crate::{
    Namespace, RepoFacts, Tier, Token,
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
    if !looks_like_path(candidate) {
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
                suggestion: referent,
                note: "文件存在，但源码行号或符号定位无法确定性确认".to_owned(),
                searched: vec!["source locator".into()],
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
                suggestion: referent,
                note: "文件存在，但源码行号或符号定位无法确定性确认".to_owned(),
                searched: vec!["contextual parent path".into()],
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

    let basename = Path::new(candidate)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(candidate);
    let relocated = facts.find_basename(basename);
    if let Some(suggestion) = relocated.first() {
        return Resolution::NearMiss {
            ns: Namespace::Path,
            suggestion: suggestion.clone(),
            note: "路径可能已移动".to_owned(),
            searched: vec!["doc-dir".into(), "project-root".into(), "repo-root".into()],
        };
    }
    if facts.has_go_mod() && looks_like_go_import(candidate) {
        return Resolution::NoMatch;
    }
    let parent = Path::new(candidate)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    if source_locator.is_none()
        && !parent.is_empty()
        && parent != "."
        && !facts
            .path_bases(&token.doc)
            .into_iter()
            .any(|base| facts.file_exists(&token.doc, base, parent))
    {
        return Resolution::NoMatch;
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
