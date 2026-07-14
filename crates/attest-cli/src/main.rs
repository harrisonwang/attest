mod config;
mod facts;
mod glob;

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, ExitCode},
    time::Duration,
};

use anyhow::{Context, Result};
use attest_core::{
    Anchor, Base, BaselineEntry, BinKnowledge, CheckOptions, Claim, ClaimLock, ClaimStatus,
    Finding, Namespace, RepoFacts, Report, Stats, Verdict, check_claims, check_document,
};
use clap::{Parser, Subcommand, ValueEnum};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{config::Config, facts::FsRepoFacts, glob::compile_globs};

#[derive(Debug, Parser)]
#[command(
    name = "attest",
    version,
    about = "文档回归测试：文档里声明的，仓库里必须成立"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check instruction documents against current repository facts.
    Check {
        #[arg(
            value_name = "DOC",
            help = "Documents to check; omit to use configured discovery"
        )]
        docs: Vec<PathBuf>,
        #[arg(long, value_enum, default_value = "tty", help = "Report output format")]
        format: OutputFormat,
        #[arg(
            long,
            help = "Check only documents related to changes since this Git ref"
        )]
        since: Option<String>,
        #[arg(long, help = "Include verified and silent findings in human output")]
        verbose: bool,
        #[arg(long, help = "Emit suspect findings for unquoted path-like prose")]
        strict: bool,
        #[arg(long, help = "Ignore the checked-in broken-finding baseline")]
        no_baseline: bool,
        #[arg(
            long,
            value_name = "IR.json",
            conflicts_with = "since",
            help = "Check only documents related to a vouch Commit IR change surface"
        )]
        vouch_ir: Option<PathBuf>,
    },
    /// Manage the accepted brownfield broken-finding baseline.
    Baseline {
        #[command(subcommand)]
        command: BaselineCommand,
    },
    /// Extract deterministic or author-time prose claims into claims.lock.
    Extract {
        #[arg(
            value_name = "DOC",
            help = "Documents to extract; omit to use configured discovery"
        )]
        docs: Vec<PathBuf>,
        #[arg(
            long,
            default_value = ".attest/claims.lock",
            help = "Claims lock output path"
        )]
        output: PathBuf,
        #[arg(long, help = "Use an author-time OpenAI-compatible Responses API")]
        llm: bool,
        #[arg(long, env = "ATTEST_OPENAI_MODEL", help = "Responses API model ID")]
        model: Option<String>,
        #[arg(long, env = "OPENAI_BASE_URL", help = "OpenAI-compatible API base URL")]
        base_url: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum BaselineCommand {
    /// Replace the baseline with all currently broken deterministic bindings.
    Update,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Tty,
    Json,
    Github,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BaselineFile {
    schema: String,
    entries: Vec<BaselineEntry>,
}

#[derive(Debug, Clone)]
struct OpenAiConfig {
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiConfig {
    fn from_env(model: Option<String>, base_url: Option<String>) -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").context("--llm requires OPENAI_API_KEY")?;
        if api_key.trim().is_empty() {
            anyhow::bail!("--llm requires a non-empty OPENAI_API_KEY");
        }
        let model = model.unwrap_or_else(|| "gpt-5.6-terra".into());
        if model.trim().is_empty() {
            anyhow::bail!("--llm requires a non-empty model");
        }
        Ok(Self {
            api_key,
            base_url: base_url
                .unwrap_or_else(|| "https://api.openai.com/v1".into())
                .trim_end_matches('/')
                .into(),
            model,
        })
    }
}

#[derive(Debug, Deserialize)]
struct LlmExtraction {
    claims: Vec<LlmClaim>,
}

#[derive(Debug, Deserialize)]
struct LlmClaim {
    claim: String,
    line: usize,
    anchors: Vec<LlmAnchor>,
}

#[derive(Debug, Deserialize)]
struct LlmAnchor {
    ns: Namespace,
    #[serde(rename = "ref")]
    referent: String,
}

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

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let root = cli
        .root
        .canonicalize()
        .with_context(|| format!("无效根目录 {}", cli.root.display()))?;
    let config = Config::load(&root)?;
    match cli.command.unwrap_or(Commands::Check {
        docs: Vec::new(),
        format: OutputFormat::Tty,
        since: None,
        verbose: false,
        strict: false,
        no_baseline: false,
        vouch_ir: None,
    }) {
        Commands::Check {
            docs,
            format,
            since,
            verbose,
            strict,
            no_baseline,
            vouch_ir,
        } => {
            let baseline = if no_baseline {
                HashSet::new()
            } else {
                load_baseline(&root)?
            };
            let selected_docs = if let Some(ir_path) = vouch_ir.as_deref() {
                Some(select_vouch_docs(&root, &config, docs, ir_path)?)
            } else if docs.is_empty() {
                None
            } else {
                Some(docs)
            };
            let report = run_check(
                &root,
                &config,
                selected_docs,
                since.as_deref(),
                verbose,
                strict,
                baseline,
            )?;
            render(&report, format, verbose)?;
            let failed = config.fail_on_broken
                && report
                    .findings
                    .iter()
                    .any(|finding| finding.verdict == Verdict::Broken && !finding.baseline);
            Ok(u8::from(failed))
        }
        Commands::Baseline {
            command: BaselineCommand::Update,
        } => {
            let report = run_check(&root, &config, None, None, true, false, HashSet::new())?;
            let mut entries: Vec<_> = report
                .findings
                .iter()
                .filter(|finding| finding.verdict == Verdict::Broken)
                .filter_map(Finding::baseline_key)
                .collect();
            entries.sort_by(|left, right| {
                (&left.doc, &left.token, left.ns.as_str()).cmp(&(
                    &right.doc,
                    &right.token,
                    right.ns.as_str(),
                ))
            });
            entries.dedup();
            write_baseline(&root, entries)?;
            println!("baseline updated: {} broken bindings", report.stats.broken);
            Ok(0)
        }
        Commands::Extract {
            docs,
            output,
            llm,
            model,
            base_url,
        } => {
            let llm_config = llm
                .then(|| OpenAiConfig::from_env(model, base_url))
                .transpose()?;
            let added = extract_claims(&root, &config, docs, &output, llm_config.as_ref())?;
            println!(
                "extracted {added} proposed claims into {}",
                output.display()
            );
            Ok(0)
        }
    }
}

fn extract_claims(
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

fn llm_claims(
    config: &OpenAiConfig,
    facts: &FsRepoFacts,
    doc: &str,
    markdown: &str,
) -> Result<Vec<Claim>> {
    if markdown.len() > 262_144 {
        anyhow::bail!("{doc} exceeds the 256 KiB --llm extraction limit");
    }
    let request = llm_request(&config.model, doc, markdown);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("attest/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client
        .post(format!("{}/responses", config.base_url))
        .bearer_auth(&config.api_key)
        .json(&request)
        .send()
        .with_context(|| format!("OpenAI-compatible request failed for {doc}"))?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        let summary: String = body.chars().take(1_000).collect();
        anyhow::bail!("OpenAI-compatible API returned {status}: {summary}");
    }
    parse_llm_response(&body, facts, doc, markdown)
}

fn llm_request(model: &str, doc: &str, markdown: &str) -> serde_json::Value {
    let schema = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "claims": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "claim": {"type": "string"},
                        "line": {"type": "integer", "minimum": 1},
                        "anchors": {
                            "type": "array",
                            "minItems": 1,
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "ns": {
                                        "type": "string",
                                        "enum": ["path", "script", "pkg", "cmd", "go-import", "env", "config-key", "symbol"]
                                    },
                                    "ref": {"type": "string"}
                                },
                                "required": ["ns", "ref"]
                            }
                        }
                    },
                    "required": ["claim", "line", "anchors"]
                }
            }
        },
        "required": ["claims"]
    });
    let request = serde_json::json!({
        "model": model,
        "store": false,
        "instructions": "Extract factual prose claims about the repository from the supplied Markdown. Include only claims whose truth depends on concrete repository referents. Preserve each claim in the document's language. Use 1-based source line numbers. Every anchor ref must be explicitly supported by the document; never invent a referent. For path use the repository path, for script use only the script or target name, for pkg use only the workspace package name, for cmd use only the executable name, and for other namespaces use the exact import, environment variable, config key, or symbol.",
        "input": [{
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": format!("Document: {doc}\n\n{markdown}")
            }]
        }],
        "text": {
            "format": {
                "type": "json_schema",
                "name": "attest_claims",
                "strict": true,
                "schema": schema
            }
        }
    });
    request
}

fn parse_llm_response(
    body: &str,
    facts: &FsRepoFacts,
    doc: &str,
    markdown: &str,
) -> Result<Vec<Claim>> {
    let response: serde_json::Value =
        serde_json::from_str(body).context("OpenAI-compatible API returned invalid JSON")?;
    let output_text = response
        .get("output")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("content").and_then(serde_json::Value::as_array))
        .flatten()
        .filter(|content| {
            content.get("type").and_then(serde_json::Value::as_str) == Some("output_text")
        })
        .filter_map(|content| content.get("text").and_then(serde_json::Value::as_str))
        .collect::<String>();
    if output_text.is_empty() {
        anyhow::bail!("OpenAI-compatible API returned no output_text for {doc}");
    }
    let extraction: LlmExtraction = serde_json::from_str(&output_text)
        .context("structured extraction output did not match attest_claims")?;
    let line_count = markdown.lines().count();
    Ok(extraction
        .claims
        .into_iter()
        .filter_map(|claim| {
            let claim_text = claim.claim.trim();
            if claim_text.is_empty() || claim.line == 0 || claim.line > line_count {
                return None;
            }
            let anchors = claim
                .anchors
                .iter()
                .map(|anchor| validate_anchor(facts, doc, anchor.ns, &anchor.referent))
                .collect::<Option<Vec<_>>>()?;
            (!anchors.is_empty()).then(|| Claim {
                claim: claim_text.into(),
                doc: format!("{doc}:{}", claim.line),
                status: ClaimStatus::Proposed,
                anchors,
            })
        })
        .collect())
}

fn validate_anchor(
    facts: &FsRepoFacts,
    doc: &str,
    namespace: Namespace,
    referent: &str,
) -> Option<Anchor> {
    let referent = referent.trim();
    if referent.is_empty() {
        return None;
    }
    let hash_for = |path: &str| facts.content_hash(doc, Base::RepoRoot, path);
    let hash = match namespace {
        Namespace::Path => {
            let path = facts
                .path_bases(doc)
                .into_iter()
                .find_map(|base| facts.resolve_path(doc, base, referent))?;
            return Some(Anchor {
                ns: namespace,
                hash: hash_for(&path),
                referent: path,
            });
        }
        Namespace::Script => {
            let origin = facts.script(referent)?;
            hash_for(&origin.manifest)
        }
        Namespace::Package => facts.workspace_pkg(referent).then_some(None)?,
        Namespace::Command => {
            (!matches!(facts.binary_known(referent), BinKnowledge::Unknown)).then_some(None)?
        }
        Namespace::GoImport => facts.go_import_known(referent).then_some(None)?,
        Namespace::Env | Namespace::Symbol => {
            let hit = facts.grep_word(referent)?;
            hash_for(&hit.path)
        }
        Namespace::ConfigKey => {
            let hit = facts.config_key(doc, None, referent)?;
            hash_for(&hit.path)
        }
    };
    Some(Anchor {
        ns: namespace,
        referent: referent.into(),
        hash,
    })
}

fn prose_lines(markdown: &str) -> Vec<(usize, String, String)> {
    let mut fenced = false;
    markdown
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                return None;
            }
            if fenced || line.trim().is_empty() {
                return None;
            }
            let mut output = String::with_capacity(line.len());
            let mut inline_code = false;
            for character in line.chars() {
                if character == '`' {
                    inline_code = !inline_code;
                    output.push(' ');
                } else if inline_code {
                    output.push(' ');
                } else {
                    output.push(character);
                }
            }
            Some((index + 1, line.to_owned(), output))
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

fn strict_findings(facts: &FsRepoFacts, doc: &str, markdown: &str) -> Vec<Finding> {
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

fn path_shape_regex() -> Regex {
    Regex::new(r"(?:^|[\s（(])([A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.@-]+)+/?)")
        .expect("path regex is valid")
}

fn select_vouch_docs(
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
        add_anchor_terms(&claim.anchor_snippet, &mut terms);
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

fn add_file_terms(file: &str, terms: &mut BTreeSet<String>) {
    terms.insert(file.to_owned());
    let path = Path::new(file);
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        terms.insert(name.to_owned());
    }
    if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
        && stem.len() >= 3
    {
        terms.insert(stem.to_owned());
    }
}

fn add_anchor_terms(anchor: &str, terms: &mut BTreeSet<String>) {
    add_surface_terms(anchor, terms);
}

fn add_surface_terms(surface: &str, terms: &mut BTreeSet<String>) {
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

fn claim_anchor_matches_surface(
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

fn claim_doc_path(location: &str) -> &str {
    location
        .rsplit_once(':')
        .and_then(|(doc, line)| line.parse::<usize>().ok().map(|_| doc))
        .unwrap_or(location)
}

fn normalize_doc_args(root: &Path, docs: Vec<PathBuf>) -> Vec<String> {
    docs.into_iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn run_check(
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

fn discover_docs(facts: &FsRepoFacts, config: &Config) -> Result<Vec<String>> {
    let includes = compile_globs(&config.include)?;
    let excludes = compile_globs(&config.exclude)?;
    Ok(facts
        .files()
        .iter()
        .filter(|path| includes.iter().any(|pattern| pattern.is_match(path)))
        .filter(|path| !excludes.iter().any(|pattern| pattern.is_match(path)))
        .cloned()
        .collect())
}

fn baseline_path(root: &Path) -> PathBuf {
    root.join(".attest/baseline.json")
}

fn load_claims(root: &Path) -> Result<ClaimLock> {
    let path = root.join(".attest/claims.lock");
    if !path.exists() {
        return Ok(ClaimLock::default());
    }
    load_claims_file(&path)
}

fn load_claims_file(path: &Path) -> Result<ClaimLock> {
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

fn load_baseline(root: &Path) -> Result<HashSet<BaselineEntry>> {
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

fn write_baseline(root: &Path, entries: Vec<BaselineEntry>) -> Result<()> {
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

fn render(report: &Report, format: OutputFormat, verbose: bool) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(report)?),
        OutputFormat::Github => render_github(report, verbose),
        OutputFormat::Tty => render_tty(report, verbose),
    }
    io::stdout().flush()?;
    Ok(())
}

fn render_tty(report: &Report, verbose: bool) {
    for finding in report
        .findings
        .iter()
        .filter(|finding| visible(finding, verbose))
    {
        let marker = match finding.verdict {
            Verdict::Verified => "ok",
            Verdict::Broken if finding.baseline => "base",
            Verdict::Broken => "broken",
            Verdict::Suspect => "suspect",
            Verdict::Silent => "silent",
        };
        println!(
            "{marker:>7} {}:{}:{}  `{}`",
            finding.doc, finding.line, finding.column_start, finding.token
        );
        if let Some(suggestion) = &finding.suggestion {
            println!("         {suggestion}");
        }
    }
    println!(
        "\n{} docs, {} tokens: {} verified, {} broken ({} baselined), {} suspect, {} silent",
        report.stats.docs,
        report.stats.tokens,
        report.stats.verified,
        report.stats.broken,
        report.stats.baselined,
        report.stats.suspect,
        report.stats.silent,
    );
}

fn render_github(report: &Report, verbose: bool) {
    for finding in report
        .findings
        .iter()
        .filter(|finding| visible(finding, verbose))
    {
        let level = match finding.verdict {
            Verdict::Broken if !finding.baseline => "error",
            Verdict::Suspect | Verdict::Broken => "warning",
            _ => "notice",
        };
        let title = finding
            .ns
            .map(|namespace| format!("attest {}", namespace.as_str()))
            .unwrap_or_else(|| "attest".into());
        let message = finding.suggestion.as_deref().unwrap_or(&finding.context);
        println!(
            "::{level} file={},line={},col={},endColumn={},title={}::{}",
            escape_property(&finding.doc),
            finding.line,
            finding.column_start,
            finding.column_end,
            escape_property(&title),
            escape_data(message)
        );
    }
}

fn visible(finding: &Finding, verbose: bool) -> bool {
    verbose || matches!(finding.verdict, Verdict::Broken | Verdict::Suspect)
}

fn escape_property(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("package.json"),
            r#"{"name":"fixture","scripts":{"test":"echo ok"}}"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("AGENTS.md"),
            "Run `npm run test` and `npm run missing`.",
        )
        .unwrap();
        directory
    }

    fn init_git(directory: &Path) {
        for args in [
            &["init"][..],
            &["config", "user.email", "attest@example.invalid"],
            &["config", "user.name", "attest test"],
            &["add", "."],
            &["commit", "-m", "initial"],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(directory)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

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
    fn github_escaping_handles_workflow_commands() {
        assert_eq!(escape_property("a:b,c"), "a%3Ab%2Cc");
        assert_eq!(escape_data("a%\nb"), "a%25%0Ab");
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
        let hash = facts
            .content_hash("AGENTS.md", Base::RepoRoot, "src/auth.rs")
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
            fs::write(&path, yaml).unwrap();
            assert!(
                load_claims_file(&path).is_err(),
                "accepted malformed lock:\n{yaml}"
            );
        }
    }

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

    #[test]
    fn llm_extract_uses_structured_outputs_and_rejects_unbound_anchors() {
        let directory = fixture();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::write(directory.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let facts = FsRepoFacts::collect(directory.path(), &[]).unwrap();
        let structured = serde_json::json!({
            "claims": [
                {
                    "claim": "The entry point is src/main.rs.",
                    "line": 1,
                    "anchors": [{"ns": "path", "ref": "src/main.rs"}]
                },
                {
                    "claim": "A missing file exists.",
                    "line": 2,
                    "anchors": [{"ns": "path", "ref": "src/missing.rs"}]
                },
                {
                    "claim": "Mixed anchors are invalid.",
                    "line": 2,
                    "anchors": [
                        {"ns": "path", "ref": "src/main.rs"},
                        {"ns": "symbol", "ref": "invented_symbol"}
                    ]
                }
            ]
        });
        let response_body = serde_json::json!({
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": serde_json::to_string(&structured).unwrap()
                }]
            }]
        })
        .to_string();
        let request = llm_request(
            "test-model",
            "AGENTS.md",
            "The entry point is src/main.rs.\nOther prose.\n",
        );
        assert_eq!(request["model"], "test-model");
        assert_eq!(request["text"]["format"]["type"], "json_schema");
        assert_eq!(request["text"]["format"]["strict"], true);
        let claims = parse_llm_response(
            &response_body,
            &facts,
            "AGENTS.md",
            "The entry point is src/main.rs.\nOther prose.\n",
        )
        .unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].anchors[0].referent, "src/main.rs");
        assert_eq!(claims[0].anchors[0].hash.as_ref().unwrap().len(), 64);
    }
}
