//! 作者时点的 LLM 抽取客户端。只在显式 `extract --llm` 时联网，
//! CI 和 check 永远不会走到这里；模型提出的每个锚点都要当场确定性绑定成功，
//! 否则整条 claim 不落盘。

use std::time::Duration;

use anyhow::{Context, Result};
use attest_core::{Anchor, Base, BinKnowledge, Claim, ClaimStatus, Namespace, RepoFacts};
use serde::Deserialize;

use crate::facts::FsRepoFacts;

#[derive(Debug, Clone)]
pub(crate) struct OpenAiConfig {
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiConfig {
    pub(crate) fn from_env(model: Option<String>, base_url: Option<String>) -> Result<Self> {
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

pub(crate) fn llm_claims(
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
    serde_json::json!({
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
    })
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

/// 模型只负责提议，这里负责公证：锚点当场绑不上就整条作废。
pub(crate) fn validate_anchor(
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testutil::fixture;

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
