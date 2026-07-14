//! `attest check` 的编排：选文档（全量 / --since / 显式）、跑绑定、合并 lock 复查。

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use attest_core::{
    BaselineEntry, CheckOptions, Finding, Namespace, Report, Stats, Verdict, check_claims,
    check_document, glob_match,
};

use crate::{
    config::Config,
    facts::FsRepoFacts,
    prose::{path_shape_regex, prose_lines},
    store::{claim_doc_path, load_claims},
    surface::{add_file_terms, add_surface_terms, claim_anchor_matches_surface},
};

pub(crate) fn run_check(
    root: &Path,
    config: &Config,
    explicit_docs: Option<Vec<PathBuf>>,
    since: Option<&str>,
    verbose: bool,
    strict: bool,
    baseline: HashSet<BaselineEntry>,
) -> Result<Report> {
    let explicit_scope = explicit_docs.is_some();
    let facts = FsRepoFacts::collect(root, &config.scope)?;
    let mut docs = if let Some(explicit_docs) = explicit_docs {
        normalize_doc_args(root, explicit_docs)
    } else {
        discover_docs(&facts, config)?
    };
    let since_claim_docs = if let Some(reference) = since {
        let selection = select_since_docs(root, &facts, docs, reference, !explicit_scope)?;
        docs = selection.docs;
        Some(selection.claim_docs)
    } else {
        None
    };
    docs.sort();
    docs.dedup();

    let options = CheckOptions {
        context_guard: config.context_guard,
        verbose: true,
        enabled_resolvers: config.enabled_resolvers.clone(),
        baseline,
    };
    let mut findings = Vec::new();
    let mut stats = Stats {
        docs: docs.len(),
        ..Stats::default()
    };
    for doc in &docs {
        let markdown =
            fs::read_to_string(root.join(doc)).with_context(|| format!("无法读取 {doc}"))?;
        let doc_findings = check_document(doc, &markdown, &facts, &options);
        stats.tokens += doc_findings.len();
        for finding in &doc_findings {
            stats.record(finding.verdict, finding.baseline);
        }
        findings.extend(doc_findings);
        if strict {
            let strict = strict_findings(&facts, doc, &markdown);
            stats.tokens += strict.len();
            for finding in &strict {
                stats.record(finding.verdict, false);
            }
            findings.extend(strict);
        }
    }
    let mut claims = load_claims(root)?;
    if let Some(claim_docs) = since_claim_docs {
        claims
            .claims
            .retain(|claim| claim_docs.contains(claim_doc_path(&claim.doc)));
    } else if explicit_scope {
        claims
            .claims
            .retain(|claim| docs.iter().any(|doc| doc == claim_doc_path(&claim.doc)));
    }
    let claim_findings = check_claims(&claims, &facts, &options);
    stats.tokens += claim_findings.len();
    for finding in &claim_findings {
        stats.record(finding.verdict, finding.baseline);
    }
    findings.extend(claim_findings);
    for (index, finding) in findings.iter_mut().enumerate() {
        finding.id = format!("f{}", index + 1);
    }
    if !verbose {
        findings.retain(|finding| matches!(finding.verdict, Verdict::Broken | Verdict::Suspect));
    }
    Ok(Report {
        schema: "attest.report.v1".into(),
        root: ".".into(),
        commit: current_commit(root),
        stats,
        findings,
    })
}

pub(crate) fn discover_docs(facts: &FsRepoFacts, config: &Config) -> Result<Vec<String>> {
    Ok(facts
        .files()
        .iter()
        .filter(|path| {
            config
                .include
                .iter()
                .any(|pattern| glob_match(pattern, path))
        })
        .filter(|path| {
            !config
                .exclude
                .iter()
                .any(|pattern| glob_match(pattern, path))
        })
        .cloned()
        .collect())
}

pub(crate) fn normalize_doc_args(root: &Path, docs: Vec<PathBuf>) -> Vec<String> {
    docs.into_iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

struct SinceSelection {
    docs: Vec<String>,
    claim_docs: HashSet<String>,
}

fn select_since_docs(
    root: &Path,
    facts: &FsRepoFacts,
    candidates: Vec<String>,
    reference: &str,
    unrestricted: bool,
) -> Result<SinceSelection> {
    let changed_files: HashSet<_> = changed_files(root, reference)?.into_iter().collect();
    if changed_files.is_empty() {
        return Ok(SinceSelection {
            docs: Vec::new(),
            claim_docs: HashSet::new(),
        });
    }

    let candidate_docs: HashSet<_> = candidates.iter().cloned().collect();
    let all_docs =
        changed_files.contains("attest.toml") || changed_files.contains(".attest/baseline.json");
    let mut terms = BTreeSet::new();
    for file in &changed_files {
        if !is_markdown_path(file) {
            add_file_terms(file, &mut terms);
        }
    }
    add_diff_terms(root, reference, &mut terms)?;

    let lock = load_claims(root)?;
    let all_claim_docs = all_docs || changed_files.contains(".attest/claims.lock");
    let claim_docs: HashSet<_> = lock
        .claims
        .iter()
        .filter(|claim| {
            let source_doc = claim_doc_path(&claim.doc);
            (unrestricted || candidate_docs.contains(source_doc))
                && (all_claim_docs
                    || changed_files.contains(source_doc)
                    || claim.anchors.iter().any(|anchor| {
                        claim_anchor_matches_surface(
                            anchor,
                            source_doc,
                            facts,
                            &changed_files,
                            &terms,
                        )
                    }))
        })
        .map(|claim| claim_doc_path(&claim.doc).to_owned())
        .collect();

    let docs = if all_docs {
        candidates
    } else {
        candidates
            .into_iter()
            .filter(|doc| {
                if changed_files.contains(doc) || claim_docs.contains(doc) {
                    return true;
                }
                fs::read_to_string(root.join(doc))
                    .is_ok_and(|markdown| terms.iter().any(|term| markdown.contains(term)))
            })
            .collect()
    };
    Ok(SinceSelection { docs, claim_docs })
}

fn add_diff_terms(root: &Path, reference: &str, terms: &mut BTreeSet<String>) -> Result<()> {
    let output = Command::new("git")
        .args([
            "diff",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            reference,
            "--",
        ])
        .current_dir(root)
        .output()
        .context("无法运行 git diff")?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut include_hunk = true;
    for line in String::from_utf8(output.stdout)?.lines() {
        if let Some(path) = line.strip_prefix("--- a/") {
            include_hunk = !is_markdown_path(path);
            if include_hunk {
                add_file_terms(path, terms);
            }
        } else if let Some(path) = line.strip_prefix("+++ b/") {
            include_hunk = !is_markdown_path(path);
            if include_hunk {
                add_file_terms(path, terms);
            }
        } else if (line.starts_with('+') || line.starts_with('-'))
            && !line.starts_with("+++")
            && !line.starts_with("---")
            && include_hunk
        {
            add_surface_terms(&line[1..], terms);
        }
    }
    Ok(())
}

fn is_markdown_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "md" | "mdx" | "markdown"))
}

fn changed_files(root: &Path, reference: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "-z", reference, "--"])
        .current_dir(root)
        .output()
        .context("无法运行 git diff")?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()))
        .collect::<std::result::Result<Vec<_>, _>>()?)
}

fn current_commit(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `--strict`：散文里长得像路径、又没加反引号也绑不上的词，提示为 suspect。
fn strict_findings(facts: &FsRepoFacts, doc: &str, markdown: &str) -> Vec<Finding> {
    use attest_core::RepoFacts;

    let mut findings = Vec::new();
    for (line, original, masked) in prose_lines(markdown) {
        for capture in path_shape_regex().captures_iter(&masked) {
            let candidate = capture
                .get(1)
                .expect("capture exists")
                .as_str()
                .trim_end_matches(['.', ',', ':', ';']);
            if candidate.starts_with("http")
                || candidate.contains('*')
                || facts
                    .path_bases(doc)
                    .into_iter()
                    .any(|base| facts.file_exists(doc, base, candidate))
            {
                continue;
            }
            let column = original.find(candidate).map_or(1, |index| index + 1);
            findings.push(Finding {
                id: String::new(),
                verdict: Verdict::Suspect,
                token: candidate.into(),
                doc: doc.into(),
                line,
                column_start: column,
                column_end: column + candidate.len(),
                context: original.clone(),
                ns: Some(Namespace::Path),
                tier: None,
                evidence: attest_core::BindingEvidence {
                    note: Some("strict：裸路径形状未加反引号且无法绑定".into()),
                    ..attest_core::BindingEvidence::default()
                },
                suggestion: Some(format!("确认路径后写成 `{candidate}`，或删除过时指涉")),
                baseline: false,
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{load_baseline, write_baseline};
    use crate::testutil::{fixture, init_git};
    use attest_core::Finding;

    #[test]
    fn end_to_end_report_and_baseline_are_stable() {
        let directory = fixture();
        let config = Config::default();
        let report = run_check(
            directory.path(),
            &config,
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();
        assert_eq!(report.stats.docs, 1);
        assert_eq!(report.stats.verified, 1);
        assert_eq!(report.stats.broken, 1);

        let entries = report
            .findings
            .iter()
            .filter_map(Finding::baseline_key)
            .collect::<Vec<_>>();
        write_baseline(directory.path(), entries.clone()).unwrap();
        assert_eq!(
            load_baseline(directory.path()).unwrap(),
            entries.into_iter().collect()
        );
    }

    #[test]
    fn malformed_nested_manifest_does_not_abort_scan() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("fixtures/broken")).unwrap();
        fs::write(directory.path().join("fixtures/broken/package.json"), "").unwrap();
        fs::write(directory.path().join("AGENTS.md"), "See `missing.rs`.").unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 1);
        assert_eq!(report.stats.broken, 1);
    }

    #[test]
    fn make_pattern_rule_is_collected_from_repository() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("Makefile"),
            "%: Makefile\n\t@echo $@\n",
        )
        .unwrap();
        fs::write(directory.path().join("AGENTS.md"), "Run `make html`.").unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.verified, 1);
        assert_eq!(report.stats.broken, 0);
    }

    #[test]
    fn just_recipe_with_default_args_is_collected() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("justfile"),
            "gen-grammar *args='':\n  echo {{args}}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Run `just gen-grammar html`.",
        )
        .unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.verified, 1);
        assert_eq!(report.stats.broken, 0);
    }

    #[test]
    fn gitignored_directory_reference_downgrades_to_suspect() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(".gitignore"), "runtime/\n").unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Agents drop events into `runtime/queue/`.\n",
        )
        .unwrap();
        init_git(directory.path());

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        let finding = report
            .findings
            .iter()
            .find(|finding| finding.token == "runtime/queue/")
            .unwrap();
        assert_eq!(finding.verdict, Verdict::Suspect);
        assert!(
            finding
                .evidence
                .note
                .as_deref()
                .is_some_and(|note| note.contains("ignore"))
        );
    }

    #[test]
    fn git_changed_files_supports_since_mode() {
        let directory = fixture();
        init_git(directory.path());
        fs::write(directory.path().join("AGENTS.md"), "Run `npm run test`.").unwrap();
        assert_eq!(
            changed_files(directory.path(), "HEAD").unwrap(),
            ["AGENTS.md"]
        );
    }

    #[test]
    fn since_selects_only_docs_referencing_changed_manifest_surface() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("docs")).unwrap();
        fs::write(
            directory.path().join("package.json"),
            r#"{"name":"fixture","scripts":{"lint":"echo ok"}}"#,
        )
        .unwrap();
        fs::write(directory.path().join("AGENTS.md"), "Run `npm run lint`.").unwrap();
        fs::write(directory.path().join("CLAUDE.md"), "See `docs/guide.md`.").unwrap();
        fs::write(directory.path().join("docs/guide.md"), "Guide.").unwrap();
        init_git(directory.path());

        fs::write(
            directory.path().join("package.json"),
            r#"{"name":"fixture","scripts":{}}"#,
        )
        .unwrap();
        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            Some("HEAD"),
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 1);
        assert!(report.findings.iter().any(|finding| {
            finding.doc == "AGENTS.md"
                && finding.token == "npm run lint"
                && finding.verdict == Verdict::Broken
        }));
    }

    #[test]
    fn since_markdown_change_rechecks_only_changed_doc() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("package.json"),
            r#"{"name":"fixture","scripts":{"lint":"echo ok"}}"#,
        )
        .unwrap();
        fs::write(directory.path().join("AGENTS.md"), "Run `npm run lint`.\n").unwrap();
        fs::write(
            directory.path().join("CLAUDE.md"),
            "Also run `npm run lint`.\n",
        )
        .unwrap();
        init_git(directory.path());

        fs::write(
            directory.path().join("AGENTS.md"),
            "Before merging, run `npm run lint`.\n",
        )
        .unwrap();
        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            Some("HEAD"),
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 1);
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.doc == "AGENTS.md")
        );
    }

    #[test]
    fn since_selects_claim_docs_by_changed_anchor_path() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(
            directory.path().join("src/config.rs"),
            "const VALUE: u8 = 1;\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Configuration is reviewed.\n",
        )
        .unwrap();
        fs::write(directory.path().join("CLAUDE.md"), "Unrelated notes.\n").unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            "schema: attest.claims.v1\nclaims:\n  - claim: configuration source exists\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: src/config.rs\n",
        )
        .unwrap();
        init_git(directory.path());

        fs::write(
            directory.path().join("src/config.rs"),
            "const VALUE: u8 = 2;\n",
        )
        .unwrap();
        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            Some("HEAD"),
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 1);
        assert!(report.findings.iter().any(|finding| {
            finding.doc == "AGENTS.md"
                && finding.token == "src/config.rs"
                && finding.verdict == Verdict::Verified
        }));
    }

    #[test]
    fn since_reports_deleted_claim_source_document() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::write(
            directory.path().join("package.json"),
            "{\"name\":\"fixture\"}\n",
        )
        .unwrap();
        fs::write(directory.path().join("REMOVED.md"), "Package notes.\n").unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            "schema: attest.claims.v1\nclaims:\n  - claim: package manifest exists\n    doc: REMOVED.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: package.json\n",
        )
        .unwrap();
        init_git(directory.path());
        fs::remove_file(directory.path().join("REMOVED.md")).unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            Some("HEAD"),
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 0);
        assert!(report.findings.iter().any(|finding| {
            finding.doc == "REMOVED.md"
                && finding.verdict == Verdict::Broken
                && finding.evidence.note.as_deref() == Some("approved claim 的来源文档已不存在")
        }));
    }

    #[test]
    fn since_rechecks_symbol_claim_when_its_source_file_changes() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(
            directory.path().join("src/auth.rs"),
            "pub fn validate_token() {}\nconst VERSION: u8 = 1;\n",
        )
        .unwrap();
        fs::write(directory.path().join("AGENTS.md"), "Auth is validated.\n").unwrap();
        let facts = FsRepoFacts::collect(directory.path(), &[]).unwrap();
        let hash = attest_core::RepoFacts::content_hash(
            &facts,
            "AGENTS.md",
            attest_core::Base::RepoRoot,
            "src/auth.rs",
        )
        .unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            format!(
                "schema: attest.claims.v1\nclaims:\n  - claim: tokens are validated\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: symbol\n        ref: validate_token\n        hash: {hash}\n"
            ),
        )
        .unwrap();
        init_git(directory.path());
        fs::write(
            directory.path().join("src/auth.rs"),
            "pub fn validate_token() {}\nconst VERSION: u8 = 2;\n",
        )
        .unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            Some("HEAD"),
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 1);
        assert!(report.findings.iter().any(|finding| {
            finding.token == "validate_token" && finding.verdict == Verdict::Suspect
        }));
    }

    #[test]
    fn explicit_docs_filter_claims_lock() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::write(directory.path().join("AGENTS.md"), "Agent notes.\n").unwrap();
        fs::write(directory.path().join("CLAUDE.md"), "Claude notes.\n").unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            "schema: attest.claims.v1\nclaims:\n  - claim: agent file exists\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: missing-agent.rs\n  - claim: claude file exists\n    doc: CLAUDE.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: missing-claude.rs\n",
        )
        .unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            Some(vec![PathBuf::from("AGENTS.md")]),
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert_eq!(report.stats.docs, 1);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.token == "missing-agent.rs")
        );
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.token != "missing-claude.rs")
        );
    }

    #[test]
    fn full_check_reports_deleted_claim_source_document() {
        let directory = fixture();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            "schema: attest.claims.v1\nclaims:\n  - claim: package manifest exists\n    doc: REMOVED.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: package.json\n",
        )
        .unwrap();

        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();

        assert!(report.findings.iter().any(|finding| {
            finding.doc == "REMOVED.md"
                && finding.verdict == Verdict::Broken
                && finding.evidence.note.as_deref() == Some("approved claim 的来源文档已不存在")
        }));
    }

    #[test]
    fn approved_missing_claim_fails_the_combined_report() {
        let directory = fixture();
        fs::create_dir_all(directory.path().join(".attest")).unwrap();
        fs::write(
            directory.path().join(".attest/claims.lock"),
            "schema: attest.claims.v1\nclaims:\n  - claim: old file exists\n    doc: AGENTS.md:1\n    status: approved\n    anchors:\n      - ns: path\n        ref: old/file.rs\n",
        )
        .unwrap();
        let report = run_check(
            directory.path(),
            &Config::default(),
            None,
            None,
            true,
            false,
            HashSet::new(),
        )
        .unwrap();
        assert!(report.findings.iter().any(|finding| {
            finding.token == "old/file.rs" && finding.verdict == Verdict::Broken
        }));
    }

    #[test]
    fn strict_mode_warns_only_for_unquoted_unbound_shapes() {
        let directory = fixture();
        let facts = FsRepoFacts::collect(directory.path(), &[]).unwrap();
        let findings = strict_findings(
            &facts,
            "AGENTS.md",
            "Old docs live in old/guide.md; ignore `inline/missing.md`.\n",
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].token, "old/guide.md");
        assert_eq!(findings[0].verdict, Verdict::Suspect);
    }
}
