use crate::{Namespace, RepoFacts, Tier, Token};

use super::Resolution;

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    let symbol = token.text.trim();
    if token.command.is_some() || !looks_like_symbol(symbol) {
        return Resolution::NoMatch;
    }
    facts
        .grep_word(symbol)
        .map_or(Resolution::NoMatch, |hit| Resolution::Bound {
            ns: Namespace::Symbol,
            referent: format!("{}:{}", hit.path, hit.line),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        })
}

fn looks_like_symbol(value: &str) -> bool {
    value.len() > 2
        && !matches!(
            value.to_ascii_lowercase().as_str(),
            "the" | "and" | "for" | "run" | "test" | "build" | "true" | "false"
        )
        && value.chars().all(|character| {
            character == '_'
                || character == '-'
                || character == ':'
                || character.is_ascii_alphanumeric()
        })
}
