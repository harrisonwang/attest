use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TokenSource {
    InlineCode,
    ShellFence,
    /// Markdown 链接或图片的目标。链接天然只能指向文件，所以这类 token
    /// 只走路径检查，不参与脚本、命令等其他角度的绑定尝试。
    LinkTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandToken {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub text: String,
    pub doc: String,
    pub line: usize,
    pub column_start: usize,
    pub column_end: usize,
    pub context: String,
    pub source: TokenSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Namespace {
    Path,
    Script,
    #[serde(rename = "pkg")]
    Package,
    #[serde(rename = "cmd")]
    Command,
    GoImport,
    Env,
    ConfigKey,
    Symbol,
    /// SKILL.md 的 frontmatter 元数据。元数据写坏了 skill 会静默加载失败，
    /// 对 agent 来说和死路径是同一类伤害。
    SkillMeta,
}

impl Namespace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Script => "script",
            Self::Package => "pkg",
            Self::Command => "cmd",
            Self::GoImport => "go-import",
            Self::Env => "env",
            Self::ConfigKey => "config-key",
            Self::Symbol => "symbol",
            Self::SkillMeta => "skill-meta",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Exact,
    Normalized,
    Relocated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Verified,
    Broken,
    Suspect,
    Silent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingEvidence {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub searched: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nearest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub verdict: Verdict,
    pub token: String,
    pub doc: String,
    pub line: usize,
    pub column_start: usize,
    pub column_end: usize,
    pub context: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ns: Option<Namespace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    pub evidence: BindingEvidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    pub baseline: bool,
}

impl Finding {
    pub fn baseline_key(&self) -> Option<BaselineEntry> {
        self.ns.map(|ns| BaselineEntry {
            doc: self.doc.clone(),
            token: self.token.clone(),
            ns,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BaselineEntry {
    pub doc: String,
    pub token: String,
    pub ns: Namespace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimLock {
    pub schema: String,
    #[serde(default)]
    pub claims: Vec<Claim>,
}

impl Default for ClaimLock {
    fn default() -> Self {
        Self {
            schema: claim_schema(),
            claims: Vec::new(),
        }
    }
}

fn claim_schema() -> String {
    "attest.claims.v1".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub claim: String,
    pub doc: String,
    pub status: ClaimStatus,
    pub anchors: Vec<Anchor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimStatus {
    Proposed,
    Approved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub ns: Namespace,
    #[serde(rename = "ref")]
    pub referent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub docs: usize,
    pub tokens: usize,
    pub verified: usize,
    pub broken: usize,
    pub suspect: usize,
    pub silent: usize,
    pub baselined: usize,
}

impl Stats {
    pub fn record(&mut self, verdict: Verdict, baseline: bool) {
        match verdict {
            Verdict::Verified => self.verified += 1,
            Verdict::Broken => self.broken += 1,
            Verdict::Suspect => self.suspect += 1,
            Verdict::Silent => self.silent += 1,
        }
        if baseline {
            self.baselined += 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub stats: Stats,
    pub findings: Vec<Finding>,
}
