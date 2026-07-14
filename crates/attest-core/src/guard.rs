//! 守卫层：决定一个 broken 要不要降级成 suspect。
//!
//! 公理 1 说误差只朝沉默，但守卫不是垃圾桶。这里只留几条有名有姓的规则，
//! 每条都给得出降级理由；某个仓库特有的例外一律写进语料回归，不进代码。
//!
//! 守卫只看 token 所在的那一行，不看整段上下文。窗口式匹配会让前文的
//! 一个 "when" 把后面不相干 token 的红也拉下水（在真实文档里复现过），
//! 所以范围收到当前行为止。

use std::path::Path;

use crate::{Finding, Namespace};

/// broken 降级检查的总入口。命中就返回降级理由，写进 evidence.note。
pub(crate) fn downgrade_note(finding: &Finding) -> Option<&'static str> {
    let line = finding.context.lines().last().unwrap_or("");
    if structure_guard(line) {
        return Some("结构守卫：token 在标题或表格行里，是版式不是断言");
    }
    if context_guard(line) {
        return Some("语境守卫：这句话在说否定、假设、举例，或文件是被生成、删除的");
    }
    if doc_class_guard(&finding.doc, finding.ns) {
        return Some("文档类别守卫：skill 和模板类文档常常在说别的仓库");
    }
    if finding.ns == Some(Namespace::Path) && token_shape_guard(&finding.token) {
        return Some("形状守卫：token 长得像占位符或运行时产物");
    }
    None
}

/// 结构守卫：标题和表格行是版式，不是对仓库的强断言。
fn structure_guard(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#') || trimmed.starts_with('|')
}

/// 语境守卫：token 所在句是否在做"这个东西不存在/是例子/会被造出来"的表述。
///
/// 词表故意收得很小，只留四类高置信信号，按类分组。想加词先问一句：
/// 它是普遍的语言现象，还是某个仓库的巧合？后者请写进 corpus/guard-cases.jsonl。
fn context_guard(line: &str) -> bool {
    const NEGATION: &[&str] = &[
        // 否定与禁止：句子明说这个东西不该用或不存在。
        "未配置",
        "不存在",
        "不要",
        "不得",
        "禁止",
        "避免",
        " not ",
        "n't",
        "never ",
        "do not",
        "must not",
        "not yet",
        "avoid ",
    ];
    const TENTATIVE: &[&str] = &[
        // 可选、废弃、待办：指涉物允许缺席。
        "可选",
        "已废弃",
        "暂未",
        "尚未",
        "optional",
        "deprecated",
        "todo",
    ];
    const EXAMPLE: &[&str] = &[
        // 示例与占位：作者在举例子，不在报路径。
        "例如",
        "比如",
        "诸如",
        "示例",
        "例：",
        "例:",
        "假如",
        "如果",
        "e.g.",
        "for example",
        "such as",
        " like ",
        "placeholder",
    ];
    const PRODUCED: &[&str] = &[
        // 产出与删改动作：文件是流程的结果，不是现存事实。
        "创建",
        "生成",
        "写入",
        "输出",
        "保存",
        "删除",
        "移除",
        "重命名",
        "create",
        "generate",
        "write ",
        "written ",
        "output ",
        "save",
        "delete",
        "remove",
        "rename",
        "will be",
        "cloned into",
    ];
    // 守卫看的是 token 周围的话，不是 token 自己：`src/removed.rs` 里的
    // "remove" 不算数，先把反引号里的内容遮掉。
    let masked = mask_inline_code(line);
    let lower = masked.to_lowercase();
    let plain = lower.replace(['*', '_'], "");
    [NEGATION, TENTATIVE, EXAMPLE, PRODUCED]
        .iter()
        .flat_map(|group| group.iter())
        .any(|pattern| lower.contains(pattern) || plain.contains(pattern))
}

fn mask_inline_code(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut in_code = false;
    for character in line.chars() {
        if character == '`' {
            in_code = !in_code;
            output.push(' ');
        } else if in_code {
            output.push(' ');
        } else {
            output.push(character);
        }
    }
    output
}

/// 文档类别守卫：某些文档天生在讲别的仓库。
///
/// SKILL.md 教 agent 在目标仓库里跑命令，脚本和包名多半不属于本仓库；
/// references/ 与 templates/ 目录装的是参考资料和模板，路径指向别处是常态。
fn doc_class_guard(doc: &str, ns: Option<Namespace>) -> bool {
    match ns {
        Some(Namespace::Script | Namespace::Package) => doc.ends_with("SKILL.md"),
        Some(Namespace::Path) => Path::new(doc).components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| matches!(name, "references" | "templates"))
        }),
        _ => false,
    }
}

/// 形状守卫：token 本身长得就像占位符或运行时产物。
fn token_shape_guard(token: &str) -> bool {
    transient_path(token) || placeholder_path(token)
}

/// 运行时产物：构建输出、缓存、日志这类目录，以及 *.local.* 与 sqlite 文件。
/// 仓库自己的 .gitignore 规则由 path resolver 单独查（见 RepoFacts::path_ignored）。
fn transient_path(value: &str) -> bool {
    value.contains(".local.")
        || Path::new(value)
            .extension()
            .is_some_and(|extension| extension == "sqlite")
        || value.split('/').any(|component| {
            matches!(
                component.trim_end_matches(['.', ',', ':', ';']),
                "node_modules"
                    | "target"
                    | "dist"
                    | "build"
                    | "coverage"
                    | ".worktrees"
                    | "worktrees"
                    | "tmp"
                    | "temp"
                    | "logs"
                    | "artifacts"
                    | "output"
                    | "outputs"
                    | ".env"
            )
        })
        || value.starts_with(".claude/workspace")
}

/// 占位符：YYYY-MM-DD、path/to/、foo/bar、your-xxx 这类写给人看的模板名。
fn placeholder_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with('.') && lower[1..].contains('.') && !lower.contains('/'))
        || lower.starts_with("nnnn_")
        || lower
            .split('/')
            .any(|component| component.contains("yyyy") || component.contains("xxxx"))
        || lower.contains("placeholder")
        || lower.contains("namehere")
        || lower.contains("path/to/")
        || lower.starts_with("your-")
        || lower.starts_with("your_")
        || lower
            .split('/')
            .any(|component| matches!(component, "foo" | "bar" | "example" | "sample"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_only_reads_the_token_line() {
        // 前一行的 "when" 不该影响当前行的裁决。
        let finding = Finding {
            id: "f1".into(),
            verdict: crate::Verdict::Broken,
            token: "src/removed.rs".into(),
            doc: "AGENTS.md".into(),
            line: 3,
            column_start: 1,
            column_end: 2,
            context: "Run `npm run x` when done.\nSee `src/removed.rs` here.".into(),
            ns: Some(Namespace::Path),
            tier: None,
            evidence: crate::BindingEvidence::default(),
            suggestion: None,
            baseline: false,
        };
        assert_eq!(downgrade_note(&finding), None);
    }

    #[test]
    fn each_guard_category_reports_its_own_reason() {
        let base = Finding {
            id: "f1".into(),
            verdict: crate::Verdict::Broken,
            token: "docs/missing.md".into(),
            doc: "AGENTS.md".into(),
            line: 1,
            column_start: 1,
            column_end: 2,
            context: String::new(),
            ns: Some(Namespace::Path),
            tier: None,
            evidence: crate::BindingEvidence::default(),
            suggestion: None,
            baseline: false,
        };
        let context = Finding {
            context: "运行后会生成 `docs/missing.md`。".into(),
            ..base.clone()
        };
        assert!(downgrade_note(&context).unwrap().starts_with("语境守卫"));
        let structure = Finding {
            context: "| 输出 | `docs/missing.md` |".into(),
            ..base.clone()
        };
        assert!(downgrade_note(&structure).unwrap().starts_with("结构守卫"));
        let doc_class = Finding {
            doc: "plugin/references/AGENTS.md".into(),
            context: "Docker files live in `docker/`.".into(),
            ..base.clone()
        };
        assert!(
            downgrade_note(&doc_class)
                .unwrap()
                .starts_with("文档类别守卫")
        );
        let shape = Finding {
            token: "research/YYYY-MM-DD-notes.md".into(),
            context: "Store notes under `research/YYYY-MM-DD-notes.md`.".into(),
            ..base.clone()
        };
        assert!(downgrade_note(&shape).unwrap().starts_with("形状守卫"));
        let plain = Finding {
            context: "See `docs/missing.md`.".into(),
            ..base
        };
        assert_eq!(downgrade_note(&plain), None);
    }
}
