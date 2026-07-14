use std::{collections::HashSet, path::Path};

use crate::{
    Anchor, BaselineEntry, BinKnowledge, ClaimLock, ClaimStatus, Finding, Namespace, RepoFacts,
    Tier, Token, Verdict,
    extract::extract_tokens,
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
                && guarded_missing_binding(&finding)
            {
                finding.verdict = Verdict::Suspect;
                finding.evidence.note =
                    Some("语境守卫：原文可能是规范、否定、假设、示例或临时产物".into());
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
            Some(format!("文档可能应改为 `{suggestion}`")),
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

fn guarded_missing_binding(finding: &Finding) -> bool {
    guarded_context(&finding.context)
        || finding
            .context
            .lines()
            .any(|line| line.trim_start().starts_with('|'))
        || matches!(finding.ns, Some(Namespace::Script | Namespace::Package))
            && finding.doc.ends_with("SKILL.md")
        || finding.ns == Some(Namespace::Path)
            && (finding.doc.ends_with("/references/AGENTS.md")
                || finding.doc.ends_with("/templates/AGENTS.md"))
        || finding.ns == Some(Namespace::Path)
            && (transient_path(&finding.token) || placeholder_path(&finding.token))
}

fn guarded_context(context: &str) -> bool {
    let lower = context.to_lowercase();
    let plain = lower.replace(['*', '_'], "");
    [
        "未配置",
        "不存在",
        "不要",
        "不得",
        "禁止",
        "避免",
        "可选",
        "已废弃",
        "如果",
        "若",
        "例如",
        "示例",
        "案例",
        "探针",
        "实为",
        "实际",
        "未来",
        "后期",
        "创建",
        "生成",
        "输出",
        "写入",
        "保存",
        "删除",
        "移除",
        "复制",
        "移动",
        "重命名",
        "比如",
        "诸如",
        "optional",
        "deprecated",
        "not yet",
        "not ",
        "do not",
        "don't",
        "must not",
        " not ",
        "never ",
        "avoid ",
        "todo",
        "e.g.",
        "for example",
        "por ejemplo",
        "例：",
        "例:",
        "örn.",
        "example",
        "such as",
        " like ",
        "placeholder",
        "generated",
        "generate ",
        "generates ",
        "create ",
        "created ",
        "write ",
        "written ",
        "output ",
        "save ",
        "saved ",
        "delete ",
        "remove ",
        "removed ",
        "copy ",
        "move ",
        "rename ",
        "cloned into",
        "will be ",
        "would be ",
        "should be ",
        "may be ",
        "can be ",
        " if ",
        "if ",
        " when ",
        "when ",
        " unless ",
        "unless ",
        " must ",
        "must ",
        " should ",
        "should ",
        " may ",
        "may ",
        " might ",
        "might ",
        " can ",
        "can ",
        "maintain ",
        "update ",
        "record ",
        "load ",
        "retrieve ",
        "staged",
        "runtime",
        " format",
        "object provides",
        "recognizes these",
        "gerrit-host",
        "cross-repo",
        "another repo",
        "other repo",
        "github.com/",
        "gitignored",
        "git-ignored",
        "look for",
        "check for",
        "search for",
        "pre-imported",
        "preinstalled",
        "provided by",
        "web upload",
        "package-claude-skills",
        "related skills",
        "keep ",
        "path pattern",
        "communicate via",
        "run migrations",
        "complement",
        "for the pattern",
        "for full list",
        "for the full list",
        "##### ",
        "*.",
    ]
    .iter()
    .any(|pattern| lower.contains(pattern) || plain.contains(pattern))
}

fn transient_path(value: &str) -> bool {
    value.contains(".local.")
        || Path::new(value)
            .extension()
            .is_some_and(|extension| extension == "sqlite")
        || value.split('/').any(|component| {
            matches!(
                component.trim_end_matches(['.', ',', ':', ';']),
                "node_modules"
                    | "target"
                    | "dist"
                    | "build"
                    | "coverage"
                    | ".worktrees"
                    | "worktrees"
                    | "tmp"
                    | "temp"
                    | "logs"
                    | "artifacts"
                    | "output"
                    | "outputs"
                    | ".env"
            )
        })
        || value.starts_with(".claude/workspace")
}

fn placeholder_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with('.') && lower[1..].contains('.') && !lower.contains('/'))
        || lower.starts_with("nnnn_")
        || lower
            .split('/')
            .any(|component| component.contains("yyyy") || component.contains("xxxx"))
        || lower.contains("placeholder")
        || lower.contains("namehere")
        || lower.contains("path/to/")
        || lower.starts_with("your-")
        || lower.starts_with("your_")
        || lower
            .split('/')
            .any(|component| matches!(component, "foo" | "bar" | "example" | "sample"))
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
    }

    impl RepoFacts for FakeFacts {
        fn file_exists(&self, doc: &str, base: Base, rel: &str) -> bool {
            self.resolve_path(doc, base, rel).is_some()
        }
        fn resolve_path(&self, _doc: &str, _base: Base, rel: &str) -> Option<String> {
            self.paths.contains(rel).then(|| rel.into())
        }
        fn glob_paths(&self, _doc: &str, _base: Base, pattern: &str) -> Vec<String> {
            self.paths
                .iter()
                .filter(|path| crate::resolve::wildcard_match(pattern, path))
                .cloned()
                .collect()
        }
        fn find_basename(&self, name: &str) -> Vec<String> {
            self.paths
                .iter()
                .filter(|path| path.ends_with(name))
                .cloned()
                .collect()
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
        fn tool_subcommand_known(&self, _tool: &str, _subcommand: &str) -> bool {
            true
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

    #[test]
    fn context_guard_downgrades_missing_path() {
        let findings = check_document(
            "AGENTS.md",
            "`CONVENTIONS.md` 尚未配置，可选。",
            &FakeFacts::default(),
            &CheckOptions::default(),
        );
        assert_eq!(findings[0].verdict, Verdict::Suspect);
    }

    #[test]
    fn context_guard_downgrades_negative_examples_and_generated_paths() {
        for markdown in [
            "Do not use branch prefixes such as `feat/` or `fix/`.",
            "Generated files include `node_modules/`, `dist/`, and `build/`.",
            "Write the result to `REPORT.md`.",
            "Use a file like `agent-openai.py` when describing providers.",
            "Event tests may live at `tests/test_EventNameHere.py`.",
            "Put personal overrides in `AGENTS.local.md`; it is gitignored.",
            "##### References (`references/`)",
            "Use `docs/private.md` if it is present.",
            "A recent cleanup removed `validation.sh` stubs.",
            "This generates files in `scripts/pr-status/`.",
            "Zed recognizes these files: `.github/copilot-instructions.md`.",
            "## Reference\n\n[upstream](https://github.com/example/project) is external.\n\n- `packages/removed/`",
            "**Related Skills:**\n- `discovery-process.md` — use the workflow",
            "Definitions use `.classes.ts` suffixes.",
            "This is **not strategy** (`finance/analysis`).",
            "## Message Board\n\nAgents communicate via `.agenthub/board/`: `dispatch/`.",
            "# Run migrations\n\n`npm run migrate`",
            "Corporate-finance complement: `finance/financial-analysis`.",
            "See `src/spec.types.ts` for the full list.",
            "See `basic-server-react/` for the pattern.",
            "Filename: `research/YYYY-MM-DD-XXXX-description.md`.",
            "Local state lives in `freshie/inventory.sqlite`.",
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
                    .all(|finding| finding.verdict != Verdict::Broken),
                "{markdown}"
            );
        }
        let reference = check_document(
            "plugin/references/AGENTS.md",
            "Docker files live in `docker/`.",
            &FakeFacts::default(),
            &CheckOptions::default(),
        );
        assert_eq!(reference[0].verdict, Verdict::Suspect);
        assert!(
            check_document(
                "agents/CLAUDE.md",
                "### Path Pattern\n\nUse `../../` to reach the root.",
                &FakeFacts::default(),
                &CheckOptions::default(),
            )
            .is_empty()
        );
    }

    #[test]
    fn path_resolver_uses_nearby_contextual_parent_paths() {
        let facts = FakeFacts {
            paths: HashSet::from(["src/auth/ensureAuth.ts".into(), "research/finance".into()]),
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
            paths: HashSet::from(["src/runtime/shell".into()]),
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

    #[test]
    fn reviewed_corpus_meets_path_detection_gate() {
        let cases: Vec<CorpusCase> = include_str!("../../../corpus/reviewed.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|case: &CorpusCase| case.reviewed && case.resolver == "path")
            .collect();
        assert!(cases.len() >= 200, "reviewed corpus unexpectedly shrank");
        let mut detected = 0;
        let mut undetected = Vec::new();
        for case in &cases {
            let facts = FakeFacts {
                paths: HashSet::from([case.after.clone()]),
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
            if stale.first().is_some_and(|finding| {
                matches!(finding.verdict, Verdict::Broken | Verdict::Suspect)
            }) {
                detected += 1;
            } else {
                undetected.push(case.before.clone());
            }
        }
        let coverage = detected as f64 / cases.len() as f64;
        assert!(
            coverage >= 0.80,
            "corpus coverage {:.1}% is below the P3 gate of 80%; undetected examples: {:?}",
            coverage * 100.0,
            &undetected[..undetected.len().min(20)]
        );
    }
}
