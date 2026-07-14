use crate::{
    Namespace, RepoFacts, Tier, Token,
    extract::{has_dynamic_placeholder, has_wildcard},
};

use super::{Resolution, wildcard_match};

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    let Some(command) = &token.command else {
        return Resolution::NoMatch;
    };
    let package = match command.program.as_str() {
        "cargo" => package_after_flag(&command.args, &["-p", "--package"], false),
        "pnpm" | "yarn" => package_after_flag(&command.args, &["--filter"], true),
        _ => return Resolution::NoMatch,
    };
    let Some(package) = package else {
        return Resolution::NoMatch;
    };
    if has_dynamic_placeholder(package) {
        return Resolution::Ignored;
    }
    if has_wildcard(package) {
        let mut matches = facts
            .workspace_packages()
            .into_iter()
            .filter(|name| wildcard_match(package, name))
            .collect::<Vec<_>>();
        matches.sort();
        let Some(referent) = matches.first().cloned() else {
            return Resolution::Ignored;
        };
        return Resolution::Bound {
            ns: Namespace::Package,
            referent,
            tier: Tier::Normalized,
            alternatives: matches.into_iter().skip(1).collect(),
        };
    }
    if facts.workspace_pkg(package) {
        return Resolution::Bound {
            ns: Namespace::Package,
            referent: package.to_owned(),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        };
    }
    if let Some(suggestion) = facts
        .workspace_packages()
        .into_iter()
        .find(|known| known.starts_with(package) || package.starts_with(known))
    {
        return Resolution::NearMiss {
            ns: Namespace::Package,
            suggestion: Some(suggestion),
            note: "workspace 包名可能已变化".to_owned(),
            searched: vec!["workspace manifests".into()],
            alternatives: Vec::new(),
        };
    }
    Resolution::Broken {
        ns: Namespace::Package,
        searched: vec!["workspace manifests".into()],
        suggestion: None,
    }
}

fn package_after_flag<'a>(
    args: &'a [String],
    flags: &[&str],
    options_only: bool,
) -> Option<&'a str> {
    for (index, arg) in args.iter().enumerate() {
        if flags.contains(&arg.as_str()) {
            return args.get(index + 1).map(String::as_str);
        }
        if let Some(package) = flags
            .iter()
            .find_map(|flag| arg.strip_prefix(&format!("{flag}=")))
        {
            return Some(package);
        }
        if options_only && !arg.starts_with('-') {
            return None;
        }
    }
    None
}
