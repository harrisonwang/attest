use crate::{Namespace, RepoFacts, Tier, Token};

use super::Resolution;

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    if token.command.is_some() || !looks_like_key(&token.text) {
        return Resolution::NoMatch;
    }
    facts
        .config_key(&token.doc, config_file_hint(&token.context), &token.text)
        .map_or(Resolution::NoMatch, |hit| Resolution::Bound {
            ns: Namespace::ConfigKey,
            referent: format!("{}:{}", hit.path, hit.line),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        })
}

fn config_file_hint(context: &str) -> Option<&str> {
    context
        .split(|character: char| character.is_whitespace() || "`'\"()[]{}<>,;".contains(character))
        .map(|candidate| candidate.trim_matches(['.', ':']))
        .find(|candidate| {
            std::path::Path::new(candidate)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "json" | "toml" | "yaml" | "yml"))
        })
}

fn looks_like_key(value: &str) -> bool {
    value.len() > 2
        && !value.contains(char::is_whitespace)
        && value.chars().all(|character| {
            character == '_' || character == '-' || character.is_ascii_alphanumeric()
        })
}

#[cfg(test)]
mod tests {
    use super::config_file_hint;

    #[test]
    fn extracts_nearby_config_file_hint() {
        assert_eq!(
            config_file_hint("Set `mode` in `config/settings.toml`."),
            Some("config/settings.toml")
        );
        assert_eq!(config_file_hint("Set `mode` for local runs."), None);
    }
}
