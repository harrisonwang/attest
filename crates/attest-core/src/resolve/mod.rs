mod command;
mod config_key;
mod env;
mod go_import;
mod package;
mod path;
mod script;
mod symbol;

use crate::{BindingEvidence, Namespace, RepoFacts, Tier, Token};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Resolution {
    Bound {
        ns: Namespace,
        referent: String,
        tier: Tier,
        alternatives: Vec<String>,
    },
    NearMiss {
        ns: Namespace,
        suggestion: String,
        note: String,
        searched: Vec<String>,
    },
    Broken {
        ns: Namespace,
        searched: Vec<String>,
        suggestion: Option<String>,
    },
    Ignored,
    NoMatch,
}

pub(crate) fn resolve(token: &Token, facts: &dyn RepoFacts, enabled: &[Namespace]) -> Resolution {
    type Resolver = fn(&Token, &dyn RepoFacts) -> Resolution;
    let resolvers: &[(Namespace, Resolver)] = &[
        (Namespace::Path, path::resolve),
        (Namespace::Script, script::resolve),
        (Namespace::Package, package::resolve),
        (Namespace::Command, command::resolve),
        (Namespace::GoImport, go_import::resolve),
        (Namespace::Env, env::resolve),
        (Namespace::ConfigKey, config_key::resolve),
        (Namespace::Symbol, symbol::resolve),
    ];

    let mut near_misses = Vec::new();
    for (namespace, resolver) in resolvers {
        if !enabled.contains(namespace) {
            continue;
        }
        match resolver(token, facts) {
            bound @ Resolution::Bound { .. } => return bound,
            broken @ Resolution::Broken { .. } => return broken,
            Resolution::Ignored => return Resolution::Ignored,
            near @ Resolution::NearMiss { .. } => near_misses.push(near),
            Resolution::NoMatch => {}
        }
    }
    near_misses
        .into_iter()
        .next()
        .unwrap_or(Resolution::NoMatch)
}

pub(crate) fn evidence_for(resolution: &Resolution) -> BindingEvidence {
    match resolution {
        Resolution::Bound {
            referent,
            alternatives,
            ..
        } => BindingEvidence {
            referent: Some(referent.clone()),
            alternatives: alternatives.clone(),
            ..BindingEvidence::default()
        },
        Resolution::NearMiss {
            suggestion,
            note,
            searched,
            ..
        } => BindingEvidence {
            searched: searched.clone(),
            nearest: Some(suggestion.clone()),
            note: Some(note.clone()),
            ..BindingEvidence::default()
        },
        Resolution::Broken { searched, .. } => BindingEvidence {
            searched: searched.clone(),
            ..BindingEvidence::default()
        },
        Resolution::Ignored | Resolution::NoMatch => BindingEvidence::default(),
    }
}

pub(crate) fn edit_distance(left: &str, right: &str) -> usize {
    let mut previous: Vec<usize> = (0..=right.chars().count()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.chars().enumerate() {
            current.push(std::cmp::min(
                std::cmp::min(current[right_index] + 1, previous[right_index + 1] + 1),
                previous[right_index] + usize::from(left_char != right_char),
            ));
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(0)
}

pub(crate) fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    let mut previous = vec![false; candidate.chars().count() + 1];
    previous[0] = true;
    let candidate: Vec<_> = candidate.chars().collect();
    for pattern_char in pattern.chars() {
        let mut current = vec![false; candidate.len() + 1];
        if pattern_char == '*' {
            current[0] = previous[0];
        }
        for (index, candidate_char) in candidate.iter().enumerate() {
            current[index + 1] = match pattern_char {
                '*' => previous[index + 1] || current[index],
                '?' => previous[index],
                _ => previous[index] && pattern_char == *candidate_char,
            };
        }
        previous = current;
    }
    previous.last().copied().unwrap_or(false)
}
