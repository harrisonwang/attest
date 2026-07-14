use std::fmt::Debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    DocDir,
    ProjectRoot,
    RepoRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOrigin {
    pub manifest: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinKnowledge {
    Repo { origin: String },
    Path,
    ToolTable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstHit {
    pub path: String,
    pub line: usize,
}

pub trait RepoFacts: Debug {
    fn path_bases(&self, _doc: &str) -> Vec<Base> {
        vec![Base::DocDir, Base::ProjectRoot, Base::RepoRoot]
    }
    fn file_exists(&self, doc: &str, base: Base, rel: &str) -> bool;
    fn resolve_path(&self, doc: &str, base: Base, rel: &str) -> Option<String>;
    fn glob_paths(&self, _doc: &str, _base: Base, _pattern: &str) -> Vec<String> {
        Vec::new()
    }
    fn find_basename(&self, name: &str) -> Vec<String>;
    fn script(&self, name: &str) -> Option<ScriptOrigin>;
    fn script_names(&self) -> Vec<String>;
    fn workspace_pkg(&self, name: &str) -> bool;
    fn workspace_packages(&self) -> Vec<String>;
    fn binary_known(&self, name: &str) -> BinKnowledge;
    fn tool_subcommand_known(&self, tool: &str, subcommand: &str) -> bool;
    fn tool_subcommand_replacement(&self, tool: &str, subcommand: &str) -> Option<String>;
    fn has_go_mod(&self) -> bool;
    fn go_import_known(&self, import: &str) -> bool;
    fn grep_word(&self, word: &str) -> Option<FirstHit>;
    fn config_key(&self, doc: &str, file_hint: Option<&str>, key: &str) -> Option<FirstHit>;
    fn content_hash(&self, doc: &str, base: Base, rel: &str) -> Option<String>;
}
