//! 入库状态的读写：broken 基线和 claims.lock。
//! 两个文件都是 review 过的仓库资产，解析失败一律按输入错误退出，不静默兜底。

use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};
use attest_core::{BaselineEntry, ClaimLock, Namespace};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct BaselineFile {
    pub(crate) schema: String,
    pub(crate) entries: Vec<BaselineEntry>,
}

pub(crate) fn baseline_path(root: &Path) -> PathBuf {
    root.join(".attest/baseline.json")
}

pub(crate) fn load_baseline(root: &Path) -> Result<HashSet<BaselineEntry>> {
    let path = baseline_path(root);
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let baseline: BaselineFile = serde_json::from_str(&fs::read_to_string(&path)?)
        .with_context(|| format!("无法解析 {}", path.display()))?;
    if baseline.schema != "attest.baseline.v1" {
        anyhow::bail!("不支持的 baseline schema: {}", baseline.schema);
    }
    Ok(baseline.entries.into_iter().collect())
}

pub(crate) fn write_baseline(root: &Path, entries: Vec<BaselineEntry>) -> Result<()> {
    let path = baseline_path(root);
    fs::create_dir_all(path.parent().expect("baseline has parent"))?;
    let baseline = BaselineFile {
        schema: "attest.baseline.v1".into(),
        entries,
    };
    fs::write(
        &path,
        format!("{}\n", serde_json::to_string_pretty(&baseline)?),
    )?;
    Ok(())
}

pub(crate) fn load_claims(root: &Path) -> Result<ClaimLock> {
    let path = root.join(".attest/claims.lock");
    if !path.exists() {
        return Ok(ClaimLock::default());
    }
    load_claims_file(&path)
}

pub(crate) fn load_claims_file(path: &Path) -> Result<ClaimLock> {
    let lock: ClaimLock = serde_yaml_ng::from_str(&fs::read_to_string(path)?)
        .with_context(|| format!("无法解析 {}", path.display()))?;
    if lock.schema != "attest.claims.v1" {
        anyhow::bail!("不支持的 claims schema: {}", lock.schema);
    }
    validate_claims(&lock)?;
    Ok(lock)
}

fn validate_claims(lock: &ClaimLock) -> Result<()> {
    for (claim_index, claim) in lock.claims.iter().enumerate() {
        let position = claim_index + 1;
        if claim.claim.trim().is_empty() {
            anyhow::bail!("claims[{position}].claim 不能为空");
        }
        if claim.doc.trim().is_empty() {
            anyhow::bail!("claims[{position}].doc 不能为空");
        }
        let Some((source_doc, line)) = claim.doc.rsplit_once(':') else {
            anyhow::bail!("claims[{position}].doc 必须使用 <相对路径>:<行号>");
        };
        if source_doc.is_empty()
            || Path::new(source_doc).is_absolute()
            || Path::new(source_doc)
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            anyhow::bail!("claims[{position}].doc 必须是仓库内相对路径");
        }
        if line
            .parse::<usize>()
            .ok()
            .filter(|line| *line > 0)
            .is_none()
        {
            anyhow::bail!("claims[{position}].doc 行号必须是大于 0 的整数");
        }
        if claim.anchors.is_empty() {
            anyhow::bail!("claims[{position}].anchors 不能为空");
        }
        for (anchor_index, anchor) in claim.anchors.iter().enumerate() {
            let anchor_position = anchor_index + 1;
            if anchor.referent.trim().is_empty() {
                anyhow::bail!("claims[{position}].anchors[{anchor_position}].ref 不能为空");
            }
            if let Some(hash) = &anchor.hash {
                if matches!(
                    anchor.ns,
                    Namespace::Package | Namespace::Command | Namespace::GoImport
                ) {
                    anyhow::bail!(
                        "claims[{position}].anchors[{anchor_position}].hash 不支持 {} 命名空间",
                        anchor.ns.as_str()
                    );
                }
                if !(8..=64).contains(&hash.len())
                    || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    anyhow::bail!(
                        "claims[{position}].anchors[{anchor_position}].hash 必须是 8 到 64 位十六进制字符串"
                    );
                }
            }
        }
    }
    Ok(())
}

/// 从 "<doc>:<line>" 里取出文档路径；没有行号就原样返回。
pub(crate) fn claim_doc_path(location: &str) -> &str {
    location
        .rsplit_once(':')
        .and_then(|(doc, line)| line.parse::<usize>().ok().map(|_| doc))
        .unwrap_or(location)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_lock_rejects_incomplete_or_malformed_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("claims.lock");
        for yaml in [
            "claims: []\n",
            "schema: attest.claims.v1\nclaims: []\ntypo: true\n",
            "schema: attest.claims.v1\nclaims:\n  - claim: empty anchor list\n    doc: AGENTS.md:1\n    status: approved\n    anchors: []\n",
            "schema: attest.claims.v1\nclaims:\n  - claim: invalid hash\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: src/main.rs\n        hash: not-a-hash\n",
            "schema: attest.claims.v1\nclaims:\n  - claim: meaningless hash\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: pkg\n        ref: fixture\n        hash: abcdef12\n",
            "schema: attest.claims.v1\nclaims:\n  - claim: missing line\n    doc: AGENTS.md\n    status: approved\n    anchors:\n      - ns: path\n        ref: src/main.rs\n",
            "schema: attest.claims.v1\nclaims:\n  - claim: invalid location\n    doc: AGENTS.md:0\n    status: approved\n    anchors:\n      - ns: path\n        ref: src/main.rs\n",
            "schema: attest.claims.v1\nclaims:\n  - claim: escaping source\n    doc: ../AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: src/main.rs\n",
        ] {
            std::fs::write(&path, yaml).unwrap();
            assert!(
                load_claims_file(&path).is_err(),
                "accepted malformed lock:\n{yaml}"
            );
        }
    }
}
