use crate::{Namespace, RepoFacts, Tier, Token};

use super::Resolution;

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    if token.command.is_some() || !looks_like_env(&token.text) {
        return Resolution::NoMatch;
    }
    facts
        .grep_word(&token.text)
        .map_or(Resolution::NoMatch, |hit| Resolution::Bound {
            ns: Namespace::Env,
            referent: format!("{}:{}", hit.path, hit.line),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        })
}

fn looks_like_env(value: &str) -> bool {
    value.len() >= 3
        && value.chars().all(|character| {
            character == '_' || character.is_ascii_uppercase() || character.is_ascii_digit()
        })
}
