use crate::{BinKnowledge, Namespace, RepoFacts, Tier, Token};

use super::Resolution;

pub(super) fn resolve(token: &Token, facts: &dyn RepoFacts) -> Resolution {
    let Some(command) = &token.command else {
        return Resolution::NoMatch;
    };
    let program = command.program.trim_start_matches("./");
    match facts.binary_known(program) {
        BinKnowledge::Repo { origin } => Resolution::Bound {
            ns: Namespace::Command,
            referent: origin,
            tier: Tier::Exact,
            alternatives: Vec::new(),
        },
        BinKnowledge::Path => Resolution::Bound {
            ns: Namespace::Command,
            referent: format!("PATH:{program}"),
            tier: Tier::Exact,
            alternatives: Vec::new(),
        },
        BinKnowledge::ToolTable => {
            let subcommand = command.args.iter().find(|arg| !arg.starts_with('-'));
            if let Some(subcommand) = subcommand {
                if facts.tool_subcommand_known(program, subcommand) {
                    return Resolution::Bound {
                        ns: Namespace::Command,
                        referent: format!("tool-table:{program}/{subcommand}"),
                        tier: Tier::Exact,
                        alternatives: Vec::new(),
                    };
                }
                if let Some(suggestion) = facts.tool_subcommand_replacement(program, subcommand) {
                    return Resolution::NearMiss {
                        ns: Namespace::Command,
                        suggestion: Some(suggestion),
                        note: "工具子命令可能已改名".to_owned(),
                        searched: vec![format!("tool-table:{program}")],
                        alternatives: Vec::new(),
                    };
                }
                // 工具认识、子命令不认识：表没有版本概念，不敢说错，也不能装作核实过。
                // verified 是正面担保，这里只配沉默。
                return Resolution::NoMatch;
            }
            Resolution::Bound {
                ns: Namespace::Command,
                referent: format!("tool-table:{program}"),
                tier: Tier::Exact,
                alternatives: Vec::new(),
            }
        }
        BinKnowledge::Unknown => Resolution::NoMatch,
    }
}
