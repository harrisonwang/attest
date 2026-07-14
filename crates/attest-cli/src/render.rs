//! 报告的三种渲染：TTY 给人看，JSON 给 agent 修复用，github 给 PR annotations。

use std::io::{self, Write};

use anyhow::Result;
use attest_core::{Finding, Report, Verdict};
use clap::ValueEnum;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum OutputFormat {
    Tty,
    Json,
    Github,
}

pub(crate) fn render(report: &Report, format: OutputFormat, verbose: bool) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(report)?),
        OutputFormat::Github => render_github(report, verbose),
        OutputFormat::Tty => render_tty(report, verbose),
    }
    io::stdout().flush()?;
    Ok(())
}

fn render_tty(report: &Report, verbose: bool) {
    for finding in report
        .findings
        .iter()
        .filter(|finding| visible(finding, verbose))
    {
        let marker = match finding.verdict {
            Verdict::Verified => "ok",
            Verdict::Broken if finding.baseline => "base",
            Verdict::Broken => "broken",
            Verdict::Suspect => "suspect",
            Verdict::Silent => "silent",
        };
        println!(
            "{marker:>7} {}:{}:{}  `{}`",
            finding.doc, finding.line, finding.column_start, finding.token
        );
        if let Some(suggestion) = &finding.suggestion {
            println!("         {suggestion}");
        }
    }
    println!(
        "\n{} docs, {} tokens: {} verified, {} broken ({} baselined), {} suspect, {} silent",
        report.stats.docs,
        report.stats.tokens,
        report.stats.verified,
        report.stats.broken,
        report.stats.baselined,
        report.stats.suspect,
        report.stats.silent,
    );
}

fn render_github(report: &Report, verbose: bool) {
    for finding in report
        .findings
        .iter()
        .filter(|finding| visible(finding, verbose))
    {
        let level = match finding.verdict {
            Verdict::Broken if !finding.baseline => "error",
            Verdict::Suspect | Verdict::Broken => "warning",
            _ => "notice",
        };
        let title = finding
            .ns
            .map(|namespace| format!("attest {}", namespace.as_str()))
            .unwrap_or_else(|| "attest".into());
        let message = finding.suggestion.as_deref().unwrap_or(&finding.context);
        println!(
            "::{level} file={},line={},col={},endColumn={},title={}::{}",
            escape_property(&finding.doc),
            finding.line,
            finding.column_start,
            finding.column_end,
            escape_property(&title),
            escape_data(message)
        );
    }
}

fn visible(finding: &Finding, verbose: bool) -> bool {
    verbose || matches!(finding.verdict, Verdict::Broken | Verdict::Suspect)
}

fn escape_property(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_escaping_handles_workflow_commands() {
        assert_eq!(escape_property("a:b,c"), "a%3Ab%2Cc");
        assert_eq!(escape_data("a%\nb"), "a%25%0Ab");
    }
}
