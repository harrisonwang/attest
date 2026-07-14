use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use attest_core::{Base, Namespace};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Config {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub context_guard: bool,
    pub fail_on_broken: bool,
    pub enabled_resolvers: Vec<Namespace>,
    pub scope: Vec<(String, Base)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            include: vec![
                "CLAUDE.md".into(),
                "AGENTS.md".into(),
                "**/CLAUDE.md".into(),
                "**/AGENTS.md".into(),
                "**/SKILL.md".into(),
                ".claude/**/*.md".into(),
            ],
            exclude: vec![
                "**/node_modules/**".into(),
                "**/target/**".into(),
                "**/.git/**".into(),
            ],
            context_guard: true,
            fail_on_broken: true,
            enabled_resolvers: all_resolvers(),
            scope: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    docs: RawDocs,
    resolvers: RawResolvers,
    policy: RawPolicy,
    scope: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawDocs {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawResolvers {
    path: Option<bool>,
    script: Option<bool>,
    pkg: Option<bool>,
    cmd: Option<bool>,
    #[serde(rename = "go-import")]
    go_import: Option<bool>,
    env: Option<bool>,
    #[serde(rename = "config-key")]
    config_key: Option<bool>,
    symbol: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawPolicy {
    #[serde(rename = "fail-on")]
    fail_on: Option<String>,
    #[serde(rename = "context-guard")]
    context_guard: Option<bool>,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let path = root.join("attest.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents =
            fs::read_to_string(&path).with_context(|| format!("无法读取 {}", path.display()))?;
        let raw: RawConfig =
            toml::from_str(&contents).with_context(|| format!("无法解析 {}", path.display()))?;
        let mut config = Self::default();
        if let Some(include) = raw.docs.include {
            config.include = include;
        }
        if let Some(exclude) = raw.docs.exclude {
            config.exclude = exclude;
        }
        if let Some(context_guard) = raw.policy.context_guard {
            config.context_guard = context_guard;
        }
        if let Some(fail_on) = raw.policy.fail_on {
            config.fail_on_broken = match fail_on.as_str() {
                "broken" => true,
                "never" => false,
                _ => anyhow::bail!("policy.fail-on 仅支持 `broken` 或 `never`"),
            };
        }
        let switches = [
            (Namespace::Path, raw.resolvers.path),
            (Namespace::Script, raw.resolvers.script),
            (Namespace::Package, raw.resolvers.pkg),
            (Namespace::Command, raw.resolvers.cmd),
            (Namespace::GoImport, raw.resolvers.go_import),
            (Namespace::Env, raw.resolvers.env),
            (Namespace::ConfigKey, raw.resolvers.config_key),
            (Namespace::Symbol, raw.resolvers.symbol),
        ];
        config.enabled_resolvers = switches
            .into_iter()
            .filter_map(|(namespace, enabled)| enabled.unwrap_or(true).then_some(namespace))
            .collect();
        config.scope = raw
            .scope
            .into_iter()
            .map(|(pattern, base)| {
                let base = match base.as_str() {
                    "doc-dir" => Base::DocDir,
                    "project-root" => Base::ProjectRoot,
                    "repo-root" => Base::RepoRoot,
                    _ => anyhow::bail!(
                        "scope `{pattern}` 的值必须是 doc-dir、project-root 或 repo-root"
                    ),
                };
                Ok((pattern, base))
            })
            .collect::<Result<Vec<_>>>()?;
        config.scope.sort_by(|left, right| {
            right
                .0
                .len()
                .cmp(&left.0.len())
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(config)
    }
}

fn all_resolvers() -> Vec<Namespace> {
    vec![
        Namespace::Path,
        Namespace::Script,
        Namespace::Package,
        Namespace::Command,
        Namespace::GoImport,
        Namespace::Env,
        Namespace::ConfigKey,
        Namespace::Symbol,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_scopes_are_specificity_first() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("attest.toml"),
            "[scope]\n\"docs/**\" = \"repo-root\"\n\"docs/ops/**\" = \"doc-dir\"\n",
        )
        .unwrap();

        let config = Config::load(directory.path()).unwrap();

        assert_eq!(
            config.scope,
            [
                ("docs/ops/**".into(), Base::DocDir),
                ("docs/**".into(), Base::RepoRoot),
            ]
        );
    }
}
