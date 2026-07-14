//! vouch Commit IR 定向模式：拿 IR 声明的变更面（文件 + 锚点片段）
//! 去圈出需要复查的文档和 lock claims，不用从 diff 反推。

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{
    check::{discover_docs, normalize_doc_args},
    config::Config,
    facts::FsRepoFacts,
    store::{claim_doc_path, load_claims},
    surface::{add_file_terms, add_surface_terms, claim_anchor_matches_surface},
};

#[derive(Debug, Deserialize)]
struct VouchIrInput {
    units: Vec<VouchUnitInput>,
}

#[derive(Debug, Deserialize)]
struct VouchUnitInput {
    claims: Vec<VouchClaimInput>,
}

#[derive(Debug, Deserialize)]
struct VouchClaimInput {
    file: String,
    anchor_snippet: String,
}

pub(crate) fn select_vouch_docs(
    root: &Path,
    config: &Config,
    explicit_docs: Vec<PathBuf>,
    ir_path: &Path,
) -> Result<Vec<PathBuf>> {
    let facts = FsRepoFacts::collect(root, &config.scope)?;
    let candidates = if explicit_docs.is_empty() {
        discover_docs(&facts, config)?
    } else {
        normalize_doc_args(root, explicit_docs)
    };
    let input = read_vouch_ir(ir_path)?;
    let mut changed_files = HashSet::new();
    let mut terms = BTreeSet::new();
    for claim in input.units.into_iter().flat_map(|unit| unit.claims) {
        let file = claim.file.replace('\\', "/");
        if file.trim().is_empty() || claim.anchor_snippet.trim().is_empty() {
            anyhow::bail!("vouch IR claims require non-empty file and anchor_snippet");
        }
        changed_files.insert(file.clone());
        add_file_terms(&file, &mut terms);
        add_surface_terms(&claim.anchor_snippet, &mut terms);
    }
    if changed_files.is_empty() {
        anyhow::bail!("vouch IR contains no claims");
    }

    let lock = load_claims(root)?;
    let claim_docs: HashSet<_> = lock
        .claims
        .iter()
        .filter(|claim| {
            let source_doc = claim_doc_path(&claim.doc);
            changed_files.contains(source_doc)
                || claim.anchors.iter().any(|anchor| {
                    claim_anchor_matches_surface(anchor, source_doc, &facts, &changed_files, &terms)
                })
        })
        .map(|claim| claim_doc_path(&claim.doc).to_owned())
        .collect();

    let mut selected = Vec::new();
    for doc in candidates {
        if changed_files.contains(&doc) || claim_docs.contains(&doc) {
            selected.push(PathBuf::from(doc));
            continue;
        }
        let markdown =
            fs::read_to_string(root.join(&doc)).with_context(|| format!("无法读取 {doc}"))?;
        if terms.iter().any(|term| markdown.contains(term)) {
            selected.push(PathBuf::from(doc));
        }
    }
    Ok(selected)
}

fn read_vouch_ir(path: &Path) -> Result<VouchIrInput> {
    let mut contents = String::new();
    if path == Path::new("-") {
        io::stdin().read_to_string(&mut contents)?;
    } else {
        contents = fs::read_to_string(path)
            .with_context(|| format!("无法读取 vouch IR {}", path.display()))?;
    }
    serde_json::from_str(&contents).with_context(|| format!("无法解析 vouch IR {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vouch_ir_selects_docs_from_changed_surface_terms() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(
            directory.path().join("src/auth.rs"),
            "pub fn validate_token() {}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Authentication uses `validate_token`.",
        )
        .unwrap();
        fs::write(
            directory.path().join("CLAUDE.md"),
            "Billing uses `charge_invoice`.",
        )
        .unwrap();
        let ir = directory.path().join("ir.json");
        fs::write(
            &ir,
            r#"{
              "task_summary": "validate auth",
              "units": [{
                "id": "u1",
                "subject": "validate auth",
                "intent": "reject empty tokens",
                "type": "fix",
                "claims": [{
                  "file": "src/auth.rs",
                  "anchor_snippet": "pub fn validate_token() {}"
                }]
              }]
            }"#,
        )
        .unwrap();

        let docs =
            select_vouch_docs(directory.path(), &Config::default(), Vec::new(), &ir).unwrap();
        assert_eq!(docs, [PathBuf::from("AGENTS.md")]);
    }

    #[test]
    fn vouch_ir_selects_prose_claims_by_locked_anchor() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(
            directory.path().join("src/auth.rs"),
            "pub fn validate_token() {}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Authentication is enforced before requests are accepted.\n",
        )
        .unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            "schema: attest.claims.v1\nclaims:\n  - claim: authentication is enforced\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: symbol\n        ref: validate_token\n",
        )
        .unwrap();
        let ir = directory.path().join("ir.json");
        fs::write(
            &ir,
            r#"{
              "units": [{
                "claims": [{
                  "file": "src/auth.rs",
                  "anchor_snippet": "return validate_token(request);"
                }]
              }]
            }"#,
        )
        .unwrap();

        let docs =
            select_vouch_docs(directory.path(), &Config::default(), Vec::new(), &ir).unwrap();

        assert_eq!(docs, [PathBuf::from("AGENTS.md")]);
    }
}
