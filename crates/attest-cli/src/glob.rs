use anyhow::{Context, Result};
use regex::Regex;

pub fn compile_globs(globs: &[String]) -> Result<Vec<Regex>> {
    globs.iter().map(|glob| compile_glob(glob)).collect()
}

pub fn compile_glob(glob: &str) -> Result<Regex> {
    let mut regex = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '*' if chars.get(index + 1) == Some(&'*') => {
                index += 1;
                if chars.get(index + 1) == Some(&'/') {
                    index += 1;
                    regex.push_str("(?:.*/)?");
                } else {
                    regex.push_str(".*");
                }
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            character => regex.push_str(&regex::escape(&character.to_string())),
        }
        index += 1;
    }
    regex.push('$');
    Regex::new(&regex).with_context(|| format!("无效 glob: {glob}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_star_matches_root_and_nested_paths() {
        let regex = compile_glob("**/AGENTS.md").unwrap();
        assert!(regex.is_match("AGENTS.md"));
        assert!(regex.is_match("apps/api/AGENTS.md"));
    }
}
