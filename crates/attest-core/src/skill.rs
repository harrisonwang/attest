//! SKILL.md 的 frontmatter 校验。
//!
//! skill 靠 frontmatter 里的 name 和 description 注册进 agent 的技能表。
//! 这两个字段坏了 skill 不报错，只是安静地从技能表里消失，对 agent 的
//! 伤害和死路径同类，所以归 attest 管。只把硬伤算 broken：块缺失、YAML
//! 解析失败、必填字段不在或为空。形状不合规矩（大写、超长）只提
//! suspect——宿主对形状的执行各家不一，规范也还在演化，不做死。

use std::path::Path;

use crate::{BindingEvidence, Finding, Namespace, Tier, Verdict};

/// 只校验真正会被当成 skill 加载的文档。references/ 和 templates/ 目录里的
/// SKILL.md 是资料和模板，按文档类别守卫的同一理由跳过。
pub(crate) fn applies(doc: &str) -> bool {
    let path = Path::new(doc);
    path.file_name().is_some_and(|name| name == "SKILL.md")
        && !path.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| matches!(name, "references" | "templates"))
        })
}

pub(crate) fn check_frontmatter(doc: &str, markdown: &str) -> Vec<Finding> {
    let text = markdown.trim_start_matches('\u{feff}');
    let Some(yaml) = frontmatter_block(text) else {
        let (verdict, note) = if text.trim_start().starts_with("---") {
            (Verdict::Broken, "frontmatter 没有闭合的 --- 行")
        } else {
            (
                Verdict::Broken,
                "SKILL.md 没有 frontmatter，宿主不会把它注册成 skill",
            )
        };
        return vec![finding(
            doc,
            verdict,
            "frontmatter",
            1,
            Some(note),
            Some("在文件开头用一对 --- 行包住 name 和 description".into()),
        )];
    };
    let value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(yaml) {
        Ok(value) => value,
        Err(_) => {
            return vec![finding(
                doc,
                Verdict::Broken,
                "frontmatter",
                2,
                Some("frontmatter 不是合法的 YAML"),
                Some("修正缩进或引号后重新运行".into()),
            )];
        }
    };
    let Some(mapping) = value.as_mapping() else {
        return vec![finding(
            doc,
            Verdict::Broken,
            "frontmatter",
            2,
            Some("frontmatter 不是键值结构"),
            Some("frontmatter 需要 name 和 description 两个键".into()),
        )];
    };
    if mapping
        .get("name")
        .and_then(|name| name.as_str())
        .is_some_and(placeholder_value)
    {
        // name 还是占位符，说明整份文件是模板，不是要加载的 skill。
        return Vec::new();
    }
    let mut findings = Vec::new();
    for field in ["name", "description"] {
        let line = field_line(text, field);
        match mapping.get(field) {
            None | Some(serde_yaml_ng::Value::Null) => findings.push(finding(
                doc,
                Verdict::Broken,
                field,
                line,
                Some("frontmatter 缺少这个必填字段"),
                Some(format!("补上 {field} 字段")),
            )),
            Some(serde_yaml_ng::Value::String(text)) if text.trim().is_empty() => {
                findings.push(finding(
                    doc,
                    Verdict::Broken,
                    field,
                    line,
                    Some("必填字段是空的"),
                    Some(format!("给 {field} 填上内容")),
                ))
            }
            Some(serde_yaml_ng::Value::String(text)) => {
                if field == "name" && !name_shape_ok(text) {
                    findings.push(finding(
                        doc,
                        Verdict::Suspect,
                        field,
                        line,
                        Some("name 的形状不符合 skill 规范：小写字母、数字、连字符，64 字符以内"),
                        None,
                    ));
                }
                if field == "description" && text.chars().count() > 1024 {
                    findings.push(finding(
                        doc,
                        Verdict::Suspect,
                        field,
                        line,
                        Some("description 超过 1024 字符，宿主可能截断"),
                        None,
                    ));
                }
            }
            Some(_) => findings.push(finding(
                doc,
                Verdict::Suspect,
                field,
                line,
                Some("字段不是字符串，宿主未必认"),
                None,
            )),
        }
    }
    if findings.is_empty() {
        findings.push(finding(
            doc,
            Verdict::Verified,
            "frontmatter",
            1,
            None,
            None,
        ));
    }
    findings
}

/// 取出首个 `---` 与闭合行之间的 YAML。文件不以 frontmatter 开头就返回 None。
/// 开头的 `---` 后面允许尾随空白——真实世界的 skill 里这种文件不少，
/// YAML 宿主也都认；但 `----` 或 `--- 标题` 这种就不是 frontmatter。
fn frontmatter_block(text: &str) -> Option<&str> {
    let body = text.strip_prefix("---")?;
    let first_newline = body.find('\n')?;
    if !body[..first_newline].trim_end().is_empty() {
        return None;
    }
    let body = &body[first_newline + 1..];
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return Some(&body[..offset]);
        }
        offset += line.len();
    }
    None
}

fn name_shape_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

fn placeholder_value(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("placeholder")
        || lower.contains("namehere")
        || lower.starts_with("your-")
        || lower.starts_with("your_")
        || lower == "skill-name"
}

fn field_line(text: &str, field: &str) -> usize {
    for (index, line) in text.lines().enumerate() {
        if index > 0 && matches!(line.trim_end(), "---" | "...") {
            break;
        }
        if line.trim_start().starts_with(&format!("{field}:")) {
            return index + 1;
        }
    }
    1
}

fn finding(
    doc: &str,
    verdict: Verdict,
    token: &str,
    line: usize,
    note: Option<&str>,
    suggestion: Option<String>,
) -> Finding {
    Finding {
        id: String::new(),
        verdict,
        token: token.to_owned(),
        doc: doc.to_owned(),
        line,
        column_start: 1,
        column_end: 1,
        context: String::new(),
        ns: Some(Namespace::SkillMeta),
        tier: (verdict == Verdict::Verified).then_some(Tier::Exact),
        evidence: BindingEvidence {
            searched: vec!["frontmatter".into()],
            note: note.map(str::to_owned),
            ..BindingEvidence::default()
        },
        suggestion,
        baseline: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loadable_skill_docs_apply() {
        assert!(applies("SKILL.md"));
        assert!(applies(".claude/skills/attest/SKILL.md"));
        assert!(!applies("AGENTS.md"));
        assert!(!applies("plugin/templates/SKILL.md"));
        assert!(!applies("docs/references/SKILL.md"));
    }

    #[test]
    fn hard_failures_are_broken() {
        let missing = check_frontmatter("SKILL.md", "# 标题\n正文\n");
        assert_eq!(missing[0].verdict, Verdict::Broken);
        let unclosed = check_frontmatter("SKILL.md", "---\nname: a\n");
        assert_eq!(unclosed[0].verdict, Verdict::Broken);
        let invalid = check_frontmatter("SKILL.md", "---\nname: [\n---\n");
        assert_eq!(invalid[0].verdict, Verdict::Broken);
        let empty_name = check_frontmatter("SKILL.md", "---\nname: \"\"\ndescription: d\n---\n");
        assert_eq!(empty_name.len(), 1);
        assert_eq!(empty_name[0].verdict, Verdict::Broken);
        assert_eq!(empty_name[0].token, "name");
        let no_description = check_frontmatter("SKILL.md", "---\nname: demo\n---\n");
        assert_eq!(no_description[0].verdict, Verdict::Broken);
        assert_eq!(no_description[0].token, "description");
    }

    #[test]
    fn shape_violations_stay_advisory() {
        let uppercase = check_frontmatter("SKILL.md", "---\nname: MySkill\ndescription: d\n---\n");
        assert_eq!(uppercase[0].verdict, Verdict::Suspect);
        let long_description =
            format!("---\nname: demo\ndescription: {}\n---\n", "很".repeat(1025));
        let long = check_frontmatter("SKILL.md", &long_description);
        assert_eq!(long[0].verdict, Verdict::Suspect);
    }

    #[test]
    fn trailing_whitespace_after_the_opening_dashes_still_parses() {
        // 冷启动逮住的真实形状：聚合仓库里成批的 skill 用 "--- " 开头。
        let healthy = check_frontmatter(
            "SKILL.md",
            "--- \nname: animejs-animation\ndescription: 动画库 skill\nrisk: safe\n---\n",
        );
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].verdict, Verdict::Verified);
        let rule = check_frontmatter("SKILL.md", "----\n正文\n");
        assert_eq!(rule[0].verdict, Verdict::Broken);
    }

    #[test]
    fn healthy_frontmatter_is_verified_and_templates_stay_silent() {
        let healthy = check_frontmatter(
            "SKILL.md",
            "---\nname: attest\ndescription: 检查文档引用\n---\n\n# 正文\n",
        );
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].verdict, Verdict::Verified);
        let template = check_frontmatter(
            "SKILL.md",
            "---\nname: your-skill-name\ndescription: 待填\n---\n",
        );
        assert!(template.is_empty());
    }

    #[test]
    fn field_lines_point_into_the_frontmatter() {
        let markdown = "---\nname: demo\ndescription: d\n---\nname: 正文里的假字段\n";
        assert_eq!(field_line(markdown, "name"), 2);
        assert_eq!(field_line(markdown, "description"), 3);
    }
}
