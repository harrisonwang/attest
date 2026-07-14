use crate::{
    Namespace, RepoFacts, Tier, Token,
    extract::{has_dynamic_placeholder, has_wildcard},
};

use super::{Resolution, edit_distance, wildcard_match};

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    let Some(command) = &token.command else {
        return Resolution::NoMatch;
    };
    let script = match command.program.as_str() {
        "npm" | "pnpm" | "yarn" | "bun" if command.args.first().is_some_and(|arg| arg == "run") => {
            command
                .args
                .iter()
                .skip(1)
                .find(|arg| !arg.starts_with('-'))
        }
        "make" | "just" => command.args.iter().find(|arg| !arg.starts_with('-')),
        "cargo" => command
            .args
            .iter()
            .find(|arg| !arg.starts_with('-'))
            .and_then(|arg| {
                facts
                    .script(arg)
                    .is_some_and(|origin| origin.kind == "cargo-alias")
                    .then_some(arg)
            }),
        _ => None,
    };
    let Some(script) = script else {
        return Resolution::NoMatch;
    };
    if has_dynamic_placeholder(script) {
        return Resolution::Ignored;
    }
    if has_wildcard(script) {
        let mut matches = facts
            .script_names()
            .into_iter()
            .filter(|name| wildcard_match(script, name))
            .collect::<Vec<_>>();
        matches.sort();
        let Some(name) = matches.first() else {
            return Resolution::Ignored;
        };
        let Some(origin) = facts.script(name) else {
            return Resolution::NoMatch;
        };
        return Resolution::Bound {
            ns: Namespace::Script,
            referent: format!("{}#{name}", origin.manifest),
            tier: Tier::Normalized,
            alternatives: matches.into_iter().skip(1).collect(),
        };
    }
    if script.contains('/') || std::path::Path::new(script).extension().is_some() {
        return Resolution::NoMatch;
    }
    if let Some(origin) = facts.script(script).or_else(|| {
        (command.program == "make")
            .then(|| facts.script("%"))
            .flatten()
    }) {
        return Resolution::Bound {
            ns: Namespace::Script,
            referent: format!("{}#{}", origin.manifest, script),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        };
    }
    let nearest = facts
        .script_names()
        .into_iter()
        .filter_map(|name| {
            let distance = edit_distance(script, &name);
            (distance <= 2 || name.starts_with(script) || script.starts_with(&name))
                .then_some((distance, name))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name);
    if let Some(suggestion) = nearest {
        return Resolution::NearMiss {
            ns: Namespace::Script,
            suggestion: Some(suggestion),
            note: "脚本可能已改名".to_owned(),
            searched: vec![
                "package.json#scripts".into(),
                "Makefile targets".into(),
                "justfile targets".into(),
                "cargo aliases".into(),
            ],
            alternatives: Vec::new(),
        };
    }
    Resolution::Broken {
        ns: Namespace::Script,
        searched: vec![
            "package.json#scripts".into(),
            "Makefile targets".into(),
            "justfile targets".into(),
            "cargo aliases".into(),
        ],
        suggestion: None,
    }
}
