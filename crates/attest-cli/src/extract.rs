//! `attest extract`：把散文断言收进 claims.lock。
//! 默认只收机械候选——形状提候选、绑定定生死；`--llm` 走作者时点抽取（见 llm.rs）。

use std::{collections::HashSet, fs, path::Path, path::PathBuf};

use anyhow::Result;
use attest_core::{Anchor, Base, Claim, ClaimLock, ClaimStatus, Namespace, RepoFacts};
use regex::Regex;

use crate::{
    check::discover_docs,
    config::Config,
    facts::FsRepoFacts,
    llm::{OpenAiConfig, llm_claims},
    prose::{path_shape_regex, prose_lines},
    store::load_claims_file,
};

pub(crate) fn extract_claims(
    root: &Path,
    config: &Config,
    explicit_docs: Vec<PathBuf>,
    output: &Path,
    llm: Option<&OpenAiConfig>,
) -> Result<usize> {
    let facts = FsRepoFacts::collect(root, &config.scope)?;
    let mut docs = if explicit_docs.is_empty() {
        discover_docs(&facts, config)?
    } else {
        explicit_docs
            .into_iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    };
    docs.sort();
    docs.dedup();

    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        root.join(output)
    };
    let mut lock = if output.exists() {
        load_claims_file(&output)?
    } else {
        ClaimLock::default()
    };
    let existing: HashSet<_> = lock
        .claims
        .iter()
        .map(|claim| (claim.doc.clone(), claim.claim.clone()))
        .collect();
    let mut added = 0;
    for doc in docs {
        let markdown = fs::read_to_string(root.join(&doc))?;
        let proposed = if let Some(llm) = llm {
            llm_claims(llm, &facts, &doc, &markdown)?
        } else {
            mechanical_claims(&facts, &doc, &markdown)
        };
        for claim in proposed {
            let location = claim.doc.clone();
            let claim_text = claim.claim.clone();
            if existing.contains(&(location.clone(), claim_text.clone()))
                || lock
                    .claims
                    .iter()
                    .any(|claim| claim.doc == location && claim.claim == claim_text)
            {
                continue;
            }
            lock.claims.push(claim);
            added += 1;
        }
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_yaml_ng::to_string(&lock)?)?;
    Ok(added)
}

fn mechanical_claims(facts: &FsRepoFacts, doc: &str, markdown: &str) -> Vec<Claim> {
    prose_lines(markdown)
        .into_iter()
        .filter_map(|(line, original, masked)| {
            let anchors = mechanical_anchors(facts, doc, &masked);
            (!anchors.is_empty()).then(|| Claim {
                claim: original.trim().into(),
                doc: format!("{doc}:{line}"),
                status: ClaimStatus::Proposed,
                anchors,
            })
        })
        .collect()
}

fn mechanical_anchors(facts: &FsRepoFacts, doc: &str, line: &str) -> Vec<Anchor> {
    let path_pattern = path_shape_regex();
    let env_pattern = Regex::new(r"\b[A-Z][A-Z0-9_]{2,}\b").expect("env regex is valid");
    let mut anchors = Vec::new();
    for capture in path_pattern.captures_iter(line) {
        let candidate = capture
            .get(1)
            .expect("capture exists")
            .as_str()
            .trim_end_matches(['.', ',', ':', ';']);
        if candidate.starts_with("http") || candidate.contains('*') {
            continue;
        }
        if let Some(referent) = facts
            .path_bases(doc)
            .into_iter()
            .find_map(|base| facts.resolve_path(doc, base, candidate))
        {
            anchors.push(Anchor {
                ns: Namespace::Path,
                hash: facts.content_hash(doc, Base::RepoRoot, &referent),
                referent,
            });
        }
    }
    for candidate in env_pattern.find_iter(line).map(|matched| matched.as_str()) {
        if let Some(hit) = facts.grep_word(candidate) {
            anchors.push(Anchor {
                ns: Namespace::Env,
                hash: facts.content_hash(doc, Base::RepoRoot, &hit.path),
                referent: candidate.into(),
            });
        }
    }
    anchors.sort_by(|left, right| {
        (left.ns.as_str(), &left.referent).cmp(&(right.ns.as_str(), &right.referent))
    });
    anchors.dedup_by(|left, right| left.ns == right.ns && left.referent == right.referent);
    anchors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture;

    #[test]
    fn extract_only_writes_deterministically_bound_prose_claims() {
        let directory = fixture();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(directory.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Implementation lives in src/main.rs. Inline `src/main.rs` is gold-tier.\n",
        )
        .unwrap();
        let output = PathBuf::from("claims.yml");
        assert_eq!(
            extract_claims(
                directory.path(),
                &Config::default(),
                vec![PathBuf::from("AGENTS.md")],
                &output,
                None,
            )
            .unwrap(),
            1
        );
        let lock = load_claims_file(&directory.path().join(output)).unwrap();
        assert_eq!(lock.claims.len(), 1);
        assert_eq!(lock.claims[0].anchors[0].referent, "src/main.rs");
        assert_eq!(lock.claims[0].anchors[0].hash.as_ref().unwrap().len(), 64);
    }
}
