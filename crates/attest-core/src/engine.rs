use std::collections::HashSet;

use crate::{
    Anchor, BaselineEntry, BinKnowledge, ClaimLock, ClaimStatus, Finding, Namespace, RepoFacts,
    Tier, Token, Verdict,
    extract::extract_tokens,
    guard,
    resolve::{Resolution, evidence_for, resolve},
};

#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub context_guard: bool,
    pub verbose: bool,
    pub enabled_resolvers: Vec<Namespace>,
    pub baseline: HashSet<BaselineEntry>,
}

pub fn check_claims(
    lock: &ClaimLock,
    facts: &dyn RepoFacts,
    options: &CheckOptions,
) -> Vec<Finding> {
    lock.claims
        .iter()
        .flat_map(|claim| {
            claim
                .anchors
                .iter()
                .enumerate()
                .map(move |(index, anchor)| (claim, index, anchor))
        })
        .enumerate()
        .map(|(finding_index, (claim, anchor_index, anchor))| {
            let (doc, line) = parse_doc_location(&claim.doc);
            let source_doc_exists = facts
                .resolve_path(&doc, crate::Base::RepoRoot, &doc)
                .is_some();
            let bound = source_doc_exists
                .then(|| bind_claim_anchor(anchor, &doc, facts))
                .flatten();
            let (verdict, note, suggestion, referent) =
                if !source_doc_exists && claim.status == ClaimStatus::Approved {
                    (
                        Verdict::Broken,
                        Some("approved claim 的来源文档已不存在".into()),
                        Some("恢复来源文档，或删除 claims.lock 中的过期断言".into()),
                        None,
                    )
                } else if claim.status == ClaimStatus::Proposed {
                    (
                        Verdict::Suspect,
                        Some("claim 尚未通过 git review 审批".into()),
                        None,
                        bound.as_ref().map(|(referent, _)| referent.clone()),
                    )
                } else if let Some((referent, current_hash)) = bound {
                    if anchor.hash.is_some() && current_hash.is_none() {
                        (
                            Verdict::Suspect,
                            Some("锚点仍存在，但当前内容哈希无法确定性读取".into()),
                            Some("确认锚点可读后重新运行，或重新提取锚点哈希".into()),
                            Some(referent),
                        )
                    } else if anchor.hash.as_ref().is_some_and(|expected| {
                        current_hash.as_ref().is_some_and(|current| {
                            current != expected && !current.starts_with(expected)
                        })
                    }) {
                        (
                            Verdict::Suspect,
                            Some("锚点内容已变化，断言需要复核".into()),
                            Some("复核断言后更新 claims.lock 中的锚点哈希".into()),
                            Some(referent),
                        )
                    } else {
                        (Verdict::Verified, None, None, Some(referent))
                    }
                } else {
                    (
                        Verdict::Broken,
                        Some("approved claim 的确定性锚点已不存在".into()),
                        Some("更新文档断言，或重新提取有效锚点".into()),
                        None,
                    )
                };
            let mut finding = Finding {
                id: format!("claim{}", finding_index + 1),
                verdict,
                token: anchor.referent.clone(),
                doc: doc.clone(),
                line,
                column_start: 1,
                column_end: 1,
                context: claim.claim.clone(),
                ns: Some(anchor.ns),
                tier: (verdict == Verdict::Verified).then_some(Tier::Exact),
                evidence: crate::BindingEvidence {
                    referent,
                    note,
                    searched: vec![format!("claims.lock#{}", anchor_index + 1)],
                    ..crate::BindingEvidence::default()
                },
                suggestion,
                baseline: false,
            };
            if finding.verdict == Verdict::Broken {
                finding.baseline = finding
                    .baseline_key()
                    .is_some_and(|entry| options.baseline.contains(&entry));
            }
            finding
        })
        .collect()
}

fn bind_claim_anchor(
    anchor: &Anchor,
    doc: &str,
    facts: &dyn RepoFacts,
) -> Option<(String, Option<String>)> {
    let path_and_hash = |path: String| {
        let hash = facts.content_hash(doc, crate::Base::RepoRoot, &path);
        (path, hash)
    };
    match anchor.ns {
        Namespace::Path => facts
            .resolve_path(doc, crate::Base::RepoRoot, &anchor.referent)
            .map(path_and_hash),
        Namespace::Script => facts.script(&anchor.referent).map(|origin| {
            let hash = facts.content_hash(doc, crate::Base::RepoRoot, &origin.manifest);
            (format!("{}#{}", origin.manifest, anchor.referent), hash)
        }),
        Namespace::Package => facts
            .workspace_pkg(&anchor.referent)
            .then(|| (anchor.referent.clone(), None)),
        Namespace::Command => {
            (!matches!(facts.binary_known(&anchor.referent), BinKnowledge::Unknown))
                .then(|| (anchor.referent.clone(), None))
        }
        Namespace::GoImport => facts
            .go_import_known(&anchor.referent)
            .then(|| (anchor.referent.clone(), None)),
        Namespace::Env | Namespace::Symbol => facts.grep_word(&anchor.referent).map(|hit| {
            let hash = facts.content_hash(doc, crate::Base::RepoRoot, &hit.path);
            (format!("{}:{}", hit.path, hit.line), hash)
        }),
        Namespace::ConfigKey => facts.config_key(doc, None, &anchor.referent).map(|hit| {
            let hash = facts.content_hash(doc, crate::Base::RepoRoot, &hit.path);
            (format!("{}:{}", hit.path, hit.line), hash)
        }),
    }
}

fn parse_doc_location(location: &str) -> (String, usize) {
    location
        .rsplit_once(':')
        .and_then(|(doc, line)| line.parse().ok().map(|line| (doc.to_owned(), line)))
        .unwrap_or_else(|| (location.to_owned(), 1))
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            context_guard: true,
            verbose: false,
            enabled_resolvers: vec![
                Namespace::Path,
                Namespace::Script,
                Namespace::Package,
                Namespace::Command,
                Namespace::GoImport,
                Namespace::Env,
                Namespace::ConfigKey,
                Namespace::Symbol,
            ],
            baseline: HashSet::new(),
        }
    }
}

pub fn check_document(
    doc: &str,
    markdown: &str,
    facts: &dyn RepoFacts,
    options: &CheckOptions,
) -> Vec<Finding> {
    extract_tokens(doc, markdown)
        .into_iter()
        .enumerate()
        .filter_map(|(index, token)| {
            let resolution = resolve(&token, facts, &options.enabled_resolvers);
            let mut finding = finding_from(index + 1, token, resolution);
            if finding.verdict == Verdict::Broken
                && options.context_guard
                && let Some(note) = guard::downgrade_note(&finding)
            {
                finding.verdict = Verdict::Suspect;
                finding.evidence.note = Some(note.into());
            }
            if finding.verdict == Verdict::Broken {
                finding.baseline = finding
                    .baseline_key()
                    .is_some_and(|entry| options.baseline.contains(&entry));
            }
            (options.verbose || finding.verdict != Verdict::Silent).then_some(finding)
        })
        .collect()
}

fn finding_from(index: usize, token: Token, resolution: Resolution) -> Finding {
    let (verdict, namespace, tier, suggestion) = match &resolution {
        Resolution::Bound { ns, tier, .. } => (Verdict::Verified, Some(*ns), Some(*tier), None),
        Resolution::NearMiss { ns, suggestion, .. } => (
            Verdict::Suspect,
            Some(*ns),
            None,
            suggestion
                .as_ref()
                .map(|value| format!("文档可能应改为 `{value}`")),
        ),
        Resolution::Broken { ns, suggestion, .. } => (
            Verdict::Broken,
            Some(*ns),
            None,
            suggestion
                .clone()
                .map(|value| format!("文档应改为 `{value}`")),
        ),
        Resolution::Ignored | Resolution::NoMatch => (Verdict::Silent, None, None, None),
    };
    Finding {
        id: format!("f{index}"),
        verdict,
        token: token.text,
        doc: token.doc,
        line: token.line,
        column_start: token.column_start,
        column_end: token.column_end,
        context: token.context,
        ns: namespace,
        tier,
        evidence: evidence_for(&resolution),
        suggestion,
        baseline: false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::{Base, BinKnowledge, FirstHit, RepoFacts, ScriptOrigin};
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Default)]
    struct FakeFacts {
        paths: HashSet<String>,
        scripts: HashMap<String, ScriptOrigin>,
        words: HashMap<String, FirstHit>,
        packages: HashSet<String>,
        config: HashMap<String, FirstHit>,
        go_imports: HashSet<String>,
        hashes: HashMap<String, String>,
        ignored: HashSet<String>,
    }

    /// 真实实现的文件树带着所有祖先目录，测试替身也得带，
    /// 不然"顶层目录存在才做迁移猜测"这类规则在测试里就失真了。
    fn paths_with_ancestors<const N: usize>(paths: [&str; N]) -> HashSet<String> {
        let mut set = HashSet::new();
        for path in paths {
            let mut current = path.trim_end_matches('/').to_owned();
            loop {
                set.insert(current.clone());
                match current.rfind('/') {
                    Some(index) => current.truncate(index),
                    None => break,
                }
            }
        }
        set
    }

    impl RepoFacts for FakeFacts {
        fn file_exists(&self, doc: &str, base: Base, rel: &str) -> bool {
            self.resolve_path(doc, base, rel).is_some()
        }
        fn resolve_path(&self, _doc: &str, _base: Base, rel: &str) -> Option<String> {
            let rel = rel.trim_end_matches('/');
            self.paths.contains(rel).then(|| rel.into())
        }
        fn glob_paths(&self, _doc: &str, _base: Base, pattern: &str) -> Vec<String> {
            self.paths
                .iter()
                .filter(|path| crate::glob_match(pattern, path))
                .cloned()
                .collect()
        }
        fn find_basename(&self, name: &str) -> Vec<String> {
            let mut hits: Vec<String> = self
                .paths
                .iter()
                .filter(|path| path.rsplit('/').next() == Some(name))
                .cloned()
                .collect();
            hits.sort();
            hits
        }
        fn path_ignored(&self, rel: &str) -> bool {
            self.ignored.contains(rel)
        }
        fn script(&self, name: &str) -> Option<ScriptOrigin> {
            self.scripts.get(name).cloned()
        }
        fn script_names(&self) -> Vec<String> {
            self.scripts.keys().cloned().collect()
        }
        fn workspace_pkg(&self, name: &str) -> bool {
            self.packages.contains(name)
        }
        fn workspace_packages(&self) -> Vec<String> {
            self.packages.iter().cloned().collect()
        }
        fn binary_known(&self, name: &str) -> BinKnowledge {
            matches!(name, "cargo" | "git")
                .then_some(BinKnowledge::ToolTable)
                .unwrap_or(BinKnowledge::Unknown)
        }
        fn tool_subcommand_known(&self, _tool: &str, subcommand: &str) -> bool {
            matches!(
                subcommand,
                "add" | "commit" | "status" | "push" | "test" | "clippy" | "build" | "run"
            )
        }
        fn tool_subcommand_replacement(&self, _tool: &str, _subcommand: &str) -> Option<String> {
            None
        }
        fn has_go_mod(&self) -> bool {
            !self.go_imports.is_empty()
        }
        fn go_import_known(&self, import: &str) -> bool {
            self.go_imports.contains(import)
        }
        fn grep_word(&self, word: &str) -> Option<FirstHit> {
            self.words.get(word).cloned()
        }
        fn config_key(&self, _doc: &str, _file_hint: Option<&str>, key: &str) -> Option<FirstHit> {
            self.config.get(key).cloned()
        }
        fn content_hash(&self, _doc: &str, _base: Base, _rel: &str) -> Option<String> {
            self.hashes.get(_rel).cloned()
        }
    }

    #[test]
    fn verifies_paths_and_flags_missing_scripts() {
        let facts = FakeFacts {
            paths: HashSet::from(["src/main.rs".into()]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "See `src/main.rs`, then run `pnpm run missing`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].verdict, Verdict::Verified);
        assert_eq!(findings[1].verdict, Verdict::Broken);
        assert_eq!(findings[1].ns, Some(Namespace::Script));
    }

    #[test]
    fn wildcards_bind_when_matches_exist_and_stay_silent_otherwise() {
        let facts = FakeFacts {
            paths: HashSet::from(["src/main.rs".into(), "src/lib.rs".into()]),
            scripts: HashMap::from([
                (
                    "test:unit".into(),
                    ScriptOrigin {
                        manifest: "package.json".into(),
                        kind: "package.json".into(),
                    },
                ),
                (
                    "test:e2e".into(),
                    ScriptOrigin {
                        manifest: "package.json".into(),
                        kind: "package.json".into(),
                    },
                ),
            ]),
            packages: HashSet::from(["@app/api".into(), "@app/web".into()]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Inspect `src/*.rs`, run `pnpm run test:*`, then `pnpm --filter @app/* test`.",
            &facts,
            &CheckOptions::default(),
        );

        assert_eq!(findings.len(), 3);
        assert!(
            findings
                .iter()
                .all(|finding| finding.verdict == Verdict::Verified)
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.tier == Some(Tier::Normalized))
        );
        assert!(
            check_document(
                "AGENTS.md",
                "Inspect `missing/*.rs` and run `pnpm run absent:*`.",
                &facts,
                &CheckOptions::default(),
            )
            .is_empty()
        );
        let verbose = CheckOptions {
            verbose: true,
            ..CheckOptions::default()
        };
        let ignored = check_document(
            "AGENTS.md",
            "Run `pnpm run <script>` and `pnpm --filter @absent/* test`.",
            &facts,
            &verbose,
        );
        assert_eq!(ignored.len(), 2);
        assert!(
            ignored
                .iter()
                .all(|finding| finding.verdict == Verdict::Silent && finding.ns.is_none())
        );
    }

    #[test]
    fn source_locators_bind_as_high_confidence_paths() {
        let facts = FakeFacts {
            paths: HashSet::from(["src/service.rs".into()]),
            words: HashMap::from([(
                "handle_request".into(),
                FirstHit {
                    path: "src/service.rs".into(),
                    line: 1,
                },
            )]),
            ..FakeFacts::default()
        };
        let verified = check_document(
            "AGENTS.md",
            "See `src/service.rs:handle_request()`.",
            &facts,
            &CheckOptions::default(),
        );
        let missing = check_document(
            "AGENTS.md",
            "See `src/removed.rs:40-52`.",
            &FakeFacts::default(),
            &CheckOptions::default(),
        );

        assert_eq!(verified[0].verdict, Verdict::Verified);
        assert_eq!(verified[0].ns, Some(Namespace::Path));
        assert_eq!(missing[0].verdict, Verdict::Broken);
        assert_eq!(missing[0].ns, Some(Namespace::Path));
    }

    #[derive(Deserialize)]
    struct GuardCase {
        #[serde(default = "default_case_doc")]
        doc: String,
        markdown: String,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        ignored: Vec<String>,
        expect: Vec<String>,
        why: String,
    }

    fn default_case_doc() -> String {
        "AGENTS.md".into()
    }

    /// 守卫行为全部由带标注的语料驱动，期望裁决和理由写在数据里。
    /// 想给守卫加规则或例外，先在 corpus/guard-cases.jsonl 里补案例。
    #[test]
    fn guard_corpus_cases_hold() {
        for line in include_str!("../../../corpus/guard-cases.jsonl").lines() {
            let case: GuardCase = serde_json::from_str(line).expect("guard case parses");
            let mut paths = HashSet::new();
            for path in &case.paths {
                paths.extend(paths_with_ancestors([path.as_str()]));
            }
            let facts = FakeFacts {
                paths,
                ignored: case.ignored.iter().cloned().collect(),
                ..FakeFacts::default()
            };
            let verdicts: Vec<String> =
                check_document(&case.doc, &case.markdown, &facts, &CheckOptions::default())
                    .iter()
                    .map(|finding| {
                        match finding.verdict {
                            Verdict::Verified => "verified",
                            Verdict::Broken => "broken",
                            Verdict::Suspect => "suspect",
                            Verdict::Silent => "silent",
                        }
                        .to_owned()
                    })
                    .collect();
            assert_eq!(verdicts, case.expect, "{} ({})", case.markdown, case.why);
        }
    }

    #[test]
    fn path_resolver_uses_nearby_contextual_parent_paths() {
        let facts = FakeFacts {
            paths: paths_with_ancestors(["src/auth/ensureAuth.ts", "research/finance"]),
            ..FakeFacts::default()
        };
        for markdown in [
            "2. **Authentication** (`src/auth/`)\n\n   - `ensureAuth.ts`: handles auth",
            "Research documents were added to `/research`:\n\n- `alpha.md`\n- `beta.md`\n- `gamma.md`\n- `delta.md`\n- `finance/`",
        ] {
            let findings = check_document("AGENTS.md", markdown, &facts, &CheckOptions::default());
            assert_eq!(
                findings.last().unwrap().verdict,
                Verdict::Verified,
                "{markdown}"
            );
            assert_eq!(findings.last().unwrap().tier, Some(Tier::Normalized));
        }
    }

    #[test]
    fn relocated_directory_is_suspect_with_suggestion() {
        let facts = FakeFacts {
            paths: paths_with_ancestors(["src/runtime/shell"]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "The shell lives in `src/shell/`.",
            &facts,
            &CheckOptions::default(),
        );

        assert_eq!(findings[0].verdict, Verdict::Suspect);
        assert_eq!(
            findings[0].suggestion.as_deref(),
            Some("文档可能应改为 `src/runtime/shell`")
        );
    }

    #[test]
    fn ambiguous_relocation_lists_candidates_without_a_suggestion() {
        let facts = FakeFacts {
            paths: paths_with_ancestors(["src/runtime/shell/mod.rs", "src/legacy/shell/mod.rs"]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "The shell lives in `src/shell/`.",
            &facts,
            &CheckOptions::default(),
        );

        assert_eq!(findings[0].verdict, Verdict::Suspect);
        assert_eq!(findings[0].suggestion, None);
        assert_eq!(
            findings[0].evidence.alternatives,
            ["src/legacy/shell", "src/runtime/shell"]
        );
    }

    #[test]
    fn repo_slug_without_local_anchor_stays_silent() {
        // owner/repo 这类两段名，顶层目录在仓库里不存在，就不做迁移猜测。
        let facts = FakeFacts {
            paths: paths_with_ancestors(["distribution/attest-action/action.yml"]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Install `harrisonwang/attest-action` from GitHub.",
            &facts,
            &CheckOptions::default(),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn package_flags_are_scoped_to_their_tools() {
        for markdown in [
            "Run `python3 -m unittest discover -p test_example.py`.",
            "Run `pnpm harness demo --filter scenario_name`.",
        ] {
            let findings = check_document(
                "AGENTS.md",
                markdown,
                &FakeFacts::default(),
                &CheckOptions::default(),
            );
            assert!(
                findings
                    .iter()
                    .all(|finding| finding.ns != Some(Namespace::Package)),
                "{markdown}"
            );
        }
    }

    #[test]
    fn bun_run_file_is_not_treated_as_a_package_script() {
        let findings = check_document(
            "AGENTS.md",
            "Run `bun run --conditions=browser ./src/index.ts serve`.",
            &FakeFacts::default(),
            &CheckOptions::default(),
        );
        assert!(
            findings
                .iter()
                .all(|finding| finding.ns != Some(Namespace::Script))
        );
    }

    #[test]
    fn missing_nested_path_requires_an_existing_parent() {
        let findings = check_document(
            "AGENTS.md",
            "Read `invented/missing.md`.",
            &FakeFacts::default(),
            &CheckOptions::default(),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn extensionless_source_import_resolves_to_source_file() {
        let facts = FakeFacts {
            paths: HashSet::from(["./native-request.ts".into()]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Import `./native-request`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(findings[0].verdict, Verdict::Verified);
        assert_eq!(findings[0].tier, Some(Tier::Normalized));
    }

    #[test]
    fn urls_and_title_case_symbols_are_not_paths() {
        for markdown in ["Connect to `ws://localhost`.", "Use `Schema.Json`."] {
            let findings = check_document(
                "AGENTS.md",
                markdown,
                &FakeFacts::default(),
                &CheckOptions::default(),
            );
            assert!(
                findings
                    .iter()
                    .all(|finding| finding.ns != Some(Namespace::Path)),
                "{markdown}"
            );
        }
    }

    #[test]
    fn script_near_miss_is_only_suspect() {
        let facts = FakeFacts {
            scripts: HashMap::from([(
                "test:e2e-ci".into(),
                ScriptOrigin {
                    manifest: "package.json".into(),
                    kind: "npm".into(),
                },
            )]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Run `pnpm run test:e2e`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(findings[0].verdict, Verdict::Suspect);
        assert_eq!(findings[0].evidence.nearest.as_deref(), Some("test:e2e-ci"));
    }

    #[test]
    fn make_pattern_rule_binds_arbitrary_target() {
        let facts = FakeFacts {
            scripts: HashMap::from([(
                "%".into(),
                ScriptOrigin {
                    manifest: "docs/Makefile".into(),
                    kind: "make".into(),
                },
            )]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Run `make html`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(findings[0].verdict, Verdict::Verified);
    }

    #[test]
    fn binds_workspace_package_from_filter() {
        let facts = FakeFacts {
            packages: HashSet::from(["@attest/core".into()]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Run `pnpm --filter @attest/core test`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(findings[0].verdict, Verdict::Verified);
        assert_eq!(findings[0].ns, Some(Namespace::Package));
    }

    #[test]
    fn binds_tool_table_commands() {
        for command in ["cargo test", "cargo clippy --workspace --all-targets"] {
            let findings = check_document(
                "AGENTS.md",
                &format!("Run `{command}`."),
                &FakeFacts::default(),
                &CheckOptions::default(),
            );
            assert_eq!(findings[0].verdict, Verdict::Verified);
            assert_eq!(findings[0].ns, Some(Namespace::Command));
        }
    }

    #[test]
    fn binds_go_imports_before_they_can_be_paths() {
        let facts = FakeFacts {
            paths: HashSet::from(["src/service".into()]),
            go_imports: HashSet::from(["fmt".into(), "net/http".into()]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Use `fmt`, `net/http`, and local path `src/service`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(findings.len(), 3);
        assert!(
            findings[..2]
                .iter()
                .all(|finding| finding.verdict == Verdict::Verified
                    && finding.ns == Some(Namespace::GoImport))
        );
        assert_eq!(findings[2].ns, Some(Namespace::Path));
    }

    #[test]
    fn binds_env_config_and_symbol_namespaces() {
        let facts = FakeFacts {
            words: HashMap::from([
                (
                    "DATABASE_URL".into(),
                    FirstHit {
                        path: "src/config.rs".into(),
                        line: 4,
                    },
                ),
                (
                    "check_document".into(),
                    FirstHit {
                        path: "src/lib.rs".into(),
                        line: 8,
                    },
                ),
            ]),
            config: HashMap::from([(
                "ask_audience".into(),
                FirstHit {
                    path: ".scaffold-docs.yml".into(),
                    line: 2,
                },
            )]),
            ..FakeFacts::default()
        };
        let findings = check_document(
            "AGENTS.md",
            "Use `DATABASE_URL`, `ask_audience`, and `check_document`.",
            &facts,
            &CheckOptions::default(),
        );
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.ns)
                .collect::<Vec<_>>(),
            [
                Some(Namespace::Env),
                Some(Namespace::ConfigKey),
                Some(Namespace::Symbol)
            ]
        );
    }

    #[test]
    fn baseline_suppresses_failure_without_hiding_finding() {
        let mut options = CheckOptions::default();
        options.baseline.insert(BaselineEntry {
            doc: "AGENTS.md".into(),
            token: "missing.rs".into(),
            ns: Namespace::Path,
        });
        let findings = check_document(
            "AGENTS.md",
            "See `missing.rs`.",
            &FakeFacts::default(),
            &options,
        );
        assert_eq!(findings[0].verdict, Verdict::Broken);
        assert!(findings[0].baseline);
    }

    #[test]
    fn claims_distinguish_verified_changed_missing_and_proposed() {
        let facts = FakeFacts {
            paths: HashSet::from([
                "AGENTS.md".into(),
                "src/main.rs".into(),
                "src/lib.rs".into(),
            ]),
            hashes: HashMap::from([
                ("src/main.rs".into(), "abc123".into()),
                ("src/lib.rs".into(), "new-hash".into()),
            ]),
            ..FakeFacts::default()
        };
        let lock = crate::ClaimLock {
            schema: "attest.claims.v1".into(),
            claims: vec![
                crate::Claim {
                    claim: "main exists".into(),
                    doc: "AGENTS.md:4".into(),
                    status: crate::ClaimStatus::Approved,
                    anchors: vec![crate::Anchor {
                        ns: Namespace::Path,
                        referent: "src/main.rs".into(),
                        hash: Some("abc123".into()),
                    }],
                },
                crate::Claim {
                    claim: "lib is stable".into(),
                    doc: "AGENTS.md:5".into(),
                    status: crate::ClaimStatus::Approved,
                    anchors: vec![crate::Anchor {
                        ns: Namespace::Path,
                        referent: "src/lib.rs".into(),
                        hash: Some("old-hash".into()),
                    }],
                },
                crate::Claim {
                    claim: "old exists".into(),
                    doc: "AGENTS.md:6".into(),
                    status: crate::ClaimStatus::Approved,
                    anchors: vec![crate::Anchor {
                        ns: Namespace::Path,
                        referent: "src/old.rs".into(),
                        hash: None,
                    }],
                },
                crate::Claim {
                    claim: "proposal".into(),
                    doc: "AGENTS.md:7".into(),
                    status: crate::ClaimStatus::Proposed,
                    anchors: vec![crate::Anchor {
                        ns: Namespace::Path,
                        referent: "src/main.rs".into(),
                        hash: None,
                    }],
                },
            ],
        };
        let findings = check_claims(&lock, &facts, &CheckOptions::default());
        assert_eq!(
            findings
                .iter()
                .map(|finding| finding.verdict)
                .collect::<Vec<_>>(),
            [
                Verdict::Verified,
                Verdict::Suspect,
                Verdict::Broken,
                Verdict::Suspect
            ]
        );
        assert_eq!(findings[0].line, 4);
    }

    #[test]
    fn approved_claim_with_missing_source_doc_is_broken() {
        let facts = FakeFacts {
            paths: HashSet::from(["src/main.rs".into()]),
            ..FakeFacts::default()
        };
        let lock = crate::ClaimLock {
            schema: "attest.claims.v1".into(),
            claims: vec![crate::Claim {
                claim: "main exists".into(),
                doc: "REMOVED.md:4".into(),
                status: crate::ClaimStatus::Approved,
                anchors: vec![crate::Anchor {
                    ns: Namespace::Path,
                    referent: "src/main.rs".into(),
                    hash: None,
                }],
            }],
        };

        let findings = check_claims(&lock, &facts, &CheckOptions::default());

        assert_eq!(findings[0].verdict, Verdict::Broken);
        assert_eq!(findings[0].doc, "REMOVED.md");
        assert_eq!(
            findings[0].evidence.note.as_deref(),
            Some("approved claim 的来源文档已不存在")
        );
    }

    #[test]
    fn approved_claim_with_unreadable_expected_hash_is_suspect() {
        let facts = FakeFacts {
            paths: HashSet::from(["AGENTS.md".into(), "src/main.rs".into()]),
            ..FakeFacts::default()
        };
        let lock = crate::ClaimLock {
            schema: "attest.claims.v1".into(),
            claims: vec![crate::Claim {
                claim: "main exists".into(),
                doc: "AGENTS.md:1".into(),
                status: crate::ClaimStatus::Approved,
                anchors: vec![crate::Anchor {
                    ns: Namespace::Path,
                    referent: "src/main.rs".into(),
                    hash: Some("abcdef12".into()),
                }],
            }],
        };

        let findings = check_claims(&lock, &facts, &CheckOptions::default());

        assert_eq!(findings[0].verdict, Verdict::Suspect);
        assert_eq!(
            findings[0].evidence.note.as_deref(),
            Some("锚点仍存在，但当前内容哈希无法确定性读取")
        );
    }

    #[derive(Deserialize)]
    struct CorpusCase {
        before: String,
        after: String,
        resolver: String,
        reviewed: bool,
    }

    /// 语料门禁分两条线：broken 率衡量 CI 真能拦住多少（suspect 只警告不拦），
    /// broken+suspect 率衡量总检出。两个数字分开报，谁也不给谁凑数。
    #[test]
    fn reviewed_corpus_meets_path_detection_gates() {
        let cases: Vec<CorpusCase> = include_str!("../../../corpus/reviewed.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|case: &CorpusCase| case.reviewed && case.resolver == "path")
            .collect();
        assert!(cases.len() >= 200, "reviewed corpus unexpectedly shrank");
        let mut broken = 0;
        let mut suspect = 0;
        let mut undetected = Vec::new();
        for case in &cases {
            let facts = FakeFacts {
                paths: paths_with_ancestors([case.after.as_str()]),
                ..FakeFacts::default()
            };
            let fixed = check_document(
                "AGENTS.md",
                &format!("See `{}`.", case.after),
                &facts,
                &CheckOptions::default(),
            );
            assert_eq!(
                fixed.first().map(|finding| finding.verdict),
                Some(Verdict::Verified),
                "{}",
                case.after
            );
            let stale = check_document(
                "AGENTS.md",
                &format!("See `{}`.", case.before),
                &facts,
                &CheckOptions::default(),
            );
            match stale.first().map(|finding| finding.verdict) {
                Some(Verdict::Broken) => broken += 1,
                Some(Verdict::Suspect) => suspect += 1,
                _ => undetected.push(case.before.clone()),
            }
        }
        let broken_rate = broken as f64 / cases.len() as f64;
        let flagged_rate = (broken + suspect) as f64 / cases.len() as f64;
        assert!(
            broken_rate >= 0.55,
            "CI 拦截率（broken）{:.1}% 低于 55% 门禁；总检出 {:.1}%",
            broken_rate * 100.0,
            flagged_rate * 100.0,
        );
        assert!(
            flagged_rate >= 0.80,
            "总检出率（broken+suspect）{:.1}% 低于 80% 门禁；漏检示例：{:?}",
            flagged_rate * 100.0,
            &undetected[..undetected.len().min(20)]
        );
    }
}
