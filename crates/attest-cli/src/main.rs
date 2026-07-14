//! 命令行入口：解析参数、分发到各模块。业务逻辑都在 check / extract / vouch 里。

mod check;
mod config;
mod extract;
mod facts;
mod llm;
mod prose;
mod render;
mod store;
mod surface;
#[cfg(test)]
mod testutil;
mod vouch;

use std::{collections::HashSet, path::PathBuf, process::ExitCode};

use anyhow::{Context, Result};
use attest_core::{Finding, Verdict};
use clap::{Parser, Subcommand};

use crate::{
    check::run_check,
    config::Config,
    extract::extract_claims,
    llm::OpenAiConfig,
    render::{OutputFormat, render},
    store::{load_baseline, write_baseline},
    vouch::select_vouch_docs,
};

#[derive(Debug, Parser)]
#[command(
    name = "attest",
    version,
    about = "文档回归测试：文档里声明的，仓库里必须成立"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Check instruction documents against current repository facts.
    Check {
        #[arg(
            value_name = "DOC",
            help = "Documents to check; omit to use configured discovery"
        )]
        docs: Vec<PathBuf>,
        #[arg(long, value_enum, default_value = "tty", help = "Report output format")]
        format: OutputFormat,
        #[arg(
            long,
            help = "Check only documents related to changes since this Git ref"
        )]
        since: Option<String>,
        #[arg(long, help = "Include verified and silent findings in human output")]
        verbose: bool,
        #[arg(long, help = "Emit suspect findings for unquoted path-like prose")]
        strict: bool,
        #[arg(long, help = "Ignore the checked-in broken-finding baseline")]
        no_baseline: bool,
        #[arg(
            long,
            value_name = "IR.json",
            conflicts_with = "since",
            help = "Check only documents related to a vouch Commit IR change surface"
        )]
        vouch_ir: Option<PathBuf>,
    },
    /// Manage the accepted brownfield broken-finding baseline.
    Baseline {
        #[command(subcommand)]
        command: BaselineCommand,
    },
    /// Extract deterministic or author-time prose claims into claims.lock.
    Extract {
        #[arg(
            value_name = "DOC",
            help = "Documents to extract; omit to use configured discovery"
        )]
        docs: Vec<PathBuf>,
        #[arg(
            long,
            default_value = ".attest/claims.lock",
            help = "Claims lock output path"
        )]
        output: PathBuf,
        #[arg(long, help = "Use an author-time OpenAI-compatible Responses API")]
        llm: bool,
        #[arg(long, env = "ATTEST_OPENAI_MODEL", help = "Responses API model ID")]
        model: Option<String>,
        #[arg(long, env = "OPENAI_BASE_URL", help = "OpenAI-compatible API base URL")]
        base_url: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum BaselineCommand {
    /// Replace the baseline with all currently broken deterministic bindings.
    Update,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let root = cli
        .root
        .canonicalize()
        .with_context(|| format!("无效根目录 {}", cli.root.display()))?;
    let config = Config::load(&root)?;
    match cli.command.unwrap_or(Commands::Check {
        docs: Vec::new(),
        format: OutputFormat::Tty,
        since: None,
        verbose: false,
        strict: false,
        no_baseline: false,
        vouch_ir: None,
    }) {
        Commands::Check {
            docs,
            format,
            since,
            verbose,
            strict,
            no_baseline,
            vouch_ir,
        } => {
            let baseline = if no_baseline {
                HashSet::new()
            } else {
                load_baseline(&root)?
            };
            let selected_docs = if let Some(ir_path) = vouch_ir.as_deref() {
                Some(select_vouch_docs(&root, &config, docs, ir_path)?)
            } else if docs.is_empty() {
                None
            } else {
                Some(docs)
            };
            let report = run_check(
                &root,
                &config,
                selected_docs,
                since.as_deref(),
                verbose,
                strict,
                baseline,
            )?;
            render(&report, format, verbose)?;
            let failed = config.fail_on_broken
                && report
                    .findings
                    .iter()
                    .any(|finding| finding.verdict == Verdict::Broken && !finding.baseline);
            Ok(u8::from(failed))
        }
        Commands::Baseline {
            command: BaselineCommand::Update,
        } => {
            let report = run_check(&root, &config, None, None, true, false, HashSet::new())?;
            let mut entries: Vec<_> = report
                .findings
                .iter()
                .filter(|finding| finding.verdict == Verdict::Broken)
                .filter_map(Finding::baseline_key)
                .collect();
            entries.sort_by(|left, right| {
                (&left.doc, &left.token, left.ns.as_str()).cmp(&(
                    &right.doc,
                    &right.token,
                    right.ns.as_str(),
                ))
            });
            entries.dedup();
            write_baseline(&root, entries)?;
            println!("baseline updated: {} broken bindings", report.stats.broken);
            Ok(0)
        }
        Commands::Extract {
            docs,
            output,
            llm,
            model,
            base_url,
        } => {
            let llm_config = llm
                .then(|| OpenAiConfig::from_env(model, base_url))
                .transpose()?;
            let added = extract_claims(&root, &config, docs, &output, llm_config.as_ref())?;
            println!(
                "extracted {added} proposed claims into {}",
                output.display()
            );
            Ok(0)
        }
    }
}
