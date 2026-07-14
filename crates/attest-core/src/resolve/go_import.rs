use crate::{Namespace, RepoFacts, Tier, Token};

use super::Resolution;

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    if token.command.is_some() || !facts.has_go_mod() {
        return Resolution::NoMatch;
    }
    let import = token.text.trim();
    if import.is_empty()
        || import.contains(char::is_whitespace)
        || !import.chars().all(|character| {
            character == '/'
                || character == '.'
                || character == '-'
                || character == '_'
                || character.is_ascii_alphanumeric()
        })
    {
        return Resolution::NoMatch;
    }
    if facts.go_import_known(import) {
        Resolution::Bound {
            ns: Namespace::GoImport,
            referent: import.to_owned(),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        }
    } else {
        Resolution::NoMatch
    }
}
