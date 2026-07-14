//! 变更面到文档的关联：`--since` 和 vouch IR 定向复查共用这套词条提取。
//! 思路是宁可多查不可漏查——从变更的文件名和 diff 内容里取词，
//! 文档只要提到任何一个词就整篇重查。

use std::collections::{BTreeSet, HashSet};

use attest_core::{Anchor, Namespace, RepoFacts};

use crate::facts::FsRepoFacts;

pub(crate) fn add_file_terms(file: &str, terms: &mut BTreeSet<String>) {
    terms.insert(file.to_owned());
    let path = std::path::Path::new(file);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        terms.insert(name.to_owned());
    }
    if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
        && stem.len() >= 3
    {
        terms.insert(stem.to_owned());
    }
}

pub(crate) fn add_surface_terms(surface: &str, terms: &mut BTreeSet<String>) {
    const STOP_WORDS: &[&str] = &[
        "async",
        "await",
        "class",
        "const",
        "else",
        "false",
        "from",
        "function",
        "impl",
        "import",
        "interface",
        "let",
        "none",
        "null",
        "package",
        "private",
        "protected",
        "pub",
        "public",
        "return",
        "self",
        "some",
        "static",
        "struct",
        "true",
        "type",
        "use",
        "where",
    ];
    for term in surface.split(|character: char| {
        !(character.is_ascii_alphanumeric() || "_-./:@".contains(character))
    }) {
        let term = term.trim_matches(['.', '/', ':', '@']);
        if term.len() >= 3
            && term.len() <= 128
            && !STOP_WORDS.contains(&term.to_ascii_lowercase().as_str())
        {
            terms.insert(term.to_owned());
        }
    }
}

/// lock 里的 claim 是否被这次变更面波及：锚点词条命中，或锚点指向的文件变了。
pub(crate) fn claim_anchor_matches_surface(
    anchor: &Anchor,
    claim_doc: &str,
    facts: &FsRepoFacts,
    changed_files: &HashSet<String>,
    terms: &BTreeSet<String>,
) -> bool {
    if terms.contains(&anchor.referent) {
        return true;
    }
    match anchor.ns {
        Namespace::Path => changed_files.contains(&anchor.referent),
        Namespace::Script => facts
            .script(&anchor.referent)
            .is_some_and(|origin| changed_files.contains(&origin.manifest)),
        Namespace::Env | Namespace::Symbol => facts
            .grep_word(&anchor.referent)
            .is_some_and(|hit| changed_files.contains(&hit.path)),
        Namespace::ConfigKey => facts
            .config_key(claim_doc, None, &anchor.referent)
            .is_some_and(|hit| changed_files.contains(&hit.path)),
        _ => false,
    }
}
