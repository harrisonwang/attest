use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result};
use attest_core::{Base, BinKnowledge, FirstHit, RepoFacts, ScriptOrigin, glob_match};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

#[derive(Debug)]
pub struct FsRepoFacts {
    root: PathBuf,
    is_git: bool,
    files: Vec<String>,
    known_paths: HashSet<String>,
    basenames: HashMap<String, Vec<String>>,
    // 词 -> (files 下标, 行号)。首次查询时对可搜索文件建一次索引，
    // 之后所有 env/symbol/锚点查询都是查表，不再逐词扫全仓库。
    word_index: OnceLock<HashMap<String, (u32, u32)>>,
    ignore_hits: Mutex<HashMap<String, bool>>,
    scripts: HashMap<String, ScriptOrigin>,
    packages: HashSet<String>,
    repo_bins: HashMap<String, String>,
    go_module: Option<String>,
    go_requires: Vec<String>,
    scopes: Vec<(String, Base)>,
}

impl FsRepoFacts {
    pub fn collect(root: &Path, scope: &[(String, Base)]) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("无法解析仓库根目录 {}", root.display()))?;
        let mut files = collect_files(&root)?;
        files.sort();
        let mut known_files = collect_known_files(&root)?;
        known_files.sort();
        let mut basenames: HashMap<String, Vec<String>> = HashMap::new();
        let mut known_paths = HashSet::from([String::new()]);
        for file in &known_files {
            known_paths.insert(file.clone());
            let mut parent = Path::new(file).parent();
            while let Some(directory) = parent {
                let normalized = normalize(directory);
                if normalized.is_empty() {
                    break;
                }
                if known_paths.insert(normalized.clone())
                    && let Some(name) = directory.file_name().and_then(|name| name.to_str())
                {
                    basenames
                        .entry(name.to_owned())
                        .or_default()
                        .push(normalized);
                }
                parent = directory.parent();
            }
            if let Some(name) = Path::new(file).file_name().and_then(|name| name.to_str()) {
                basenames
                    .entry(name.to_owned())
                    .or_default()
                    .push(file.clone());
            }
        }
        for referents in basenames.values_mut() {
            referents.sort();
            referents.dedup();
        }
        let is_git = root.join(".git").exists();
        let mut facts = Self {
            root,
            is_git,
            files,
            known_paths,
            basenames,
            word_index: OnceLock::new(),
            ignore_hits: Mutex::new(HashMap::new()),
            scripts: HashMap::new(),
            packages: HashSet::new(),
            repo_bins: HashMap::new(),
            go_module: None,
            go_requires: Vec::new(),
            scopes: scope.to_vec(),
        };
        facts.collect_manifests()?;
        Ok(facts)
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    fn collect_manifests(&mut self) -> Result<()> {
        let files = self.files.clone();
        for relative in files {
            let path = self.root.join(&relative);
            match Path::new(&relative)
                .file_name()
                .and_then(|name| name.to_str())
            {
                Some("package.json") => {
                    let _ = self.collect_package_json(&relative, &path);
                }
                Some("Cargo.toml") => {
                    let _ = self.collect_cargo(&relative, &path);
                }
                Some("Makefile" | "makefile") => {
                    let _ = self.collect_makefile(&relative, &path);
                }
                Some("justfile" | "Justfile") => {
                    let _ = self.collect_justfile(&relative, &path);
                }
                Some("config.toml" | "config") if relative.contains(".cargo/") => {
                    let _ = self.collect_cargo_aliases(&relative, &path);
                }
                Some("go.mod") => {
                    let _ = self.collect_go_mod(&path);
                }
                Some("pyproject.toml") => {
                    let _ = self.collect_pyproject(&relative, &path);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn collect_package_json(&mut self, relative: &str, path: &Path) -> Result<()> {
        let contents = fs::read_to_string(path)?;
        let value: JsonValue =
            serde_json::from_str(&contents).with_context(|| format!("无法解析 {relative}"))?;
        if let Some(name) = value.get("name").and_then(JsonValue::as_str) {
            self.packages.insert(name.to_owned());
        }
        if let Some(scripts) = value.get("scripts").and_then(JsonValue::as_object) {
            for name in scripts.keys() {
                self.scripts
                    .entry(name.clone())
                    .or_insert_with(|| ScriptOrigin {
                        manifest: relative.to_owned(),
                        kind: "package.json".into(),
                    });
            }
        }
        match value.get("bin") {
            Some(JsonValue::String(_)) => {
                if let Some(name) = value.get("name").and_then(JsonValue::as_str) {
                    self.repo_bins.insert(name.to_owned(), relative.to_owned());
                }
            }
            Some(JsonValue::Object(bins)) => {
                for name in bins.keys() {
                    self.repo_bins.insert(name.clone(), relative.to_owned());
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_cargo(&mut self, relative: &str, path: &Path) -> Result<()> {
        let contents = fs::read_to_string(path)?;
        let value: toml::Value =
            toml::from_str(&contents).with_context(|| format!("无法解析 {relative}"))?;
        let package_name = value
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str);
        if let Some(name) = package_name {
            self.packages.insert(name.to_owned());
        }
        if let Some(bins) = value.get("bin").and_then(toml::Value::as_array) {
            for bin in bins {
                if let Some(name) = bin
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .or(package_name)
                {
                    self.repo_bins.insert(name.to_owned(), relative.to_owned());
                }
            }
        }
        let autobins = value
            .get("package")
            .and_then(|package| package.get("autobins"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        if autobins {
            let manifest_dir = Path::new(relative).parent().unwrap_or(Path::new(""));
            let default_main = normalize(&manifest_dir.join("src/main.rs"));
            if self.known_paths.contains(&default_main)
                && let Some(name) = package_name
            {
                self.repo_bins
                    .entry(name.to_owned())
                    .or_insert_with(|| relative.to_owned());
            }
            let bin_dir = manifest_dir.join("src/bin");
            for file in &self.files {
                let Ok(target) = Path::new(file).strip_prefix(&bin_dir) else {
                    continue;
                };
                let name = if target.components().count() == 1
                    && target.extension().and_then(|extension| extension.to_str()) == Some("rs")
                {
                    target.file_stem().and_then(|name| name.to_str())
                } else if target.components().count() == 2
                    && target.file_name().and_then(|name| name.to_str()) == Some("main.rs")
                {
                    target
                        .parent()
                        .and_then(Path::file_name)
                        .and_then(|name| name.to_str())
                } else {
                    None
                };
                if let Some(name) = name {
                    self.repo_bins
                        .entry(name.to_owned())
                        .or_insert_with(|| relative.to_owned());
                }
            }
        }
        Ok(())
    }

    fn collect_makefile(&mut self, relative: &str, path: &Path) -> Result<()> {
        for line in fs::read_to_string(path)?.lines() {
            let Some((target, _)) = line.split_once(':') else {
                continue;
            };
            if (!target.starts_with(char::is_whitespace)
                && !target.contains(['=', '$', '%'])
                && !target.is_empty())
                || target.trim() == "%"
            {
                for target in target.split_whitespace() {
                    self.scripts
                        .entry(target.into())
                        .or_insert_with(|| ScriptOrigin {
                            manifest: relative.into(),
                            kind: "make".into(),
                        });
                }
            }
        }
        Ok(())
    }

    fn collect_justfile(&mut self, relative: &str, path: &Path) -> Result<()> {
        for line in fs::read_to_string(path)?.lines() {
            let trimmed = line.trim();
            let Some((head, _)) = trimmed.split_once(':') else {
                continue;
            };
            if !line.starts_with(char::is_whitespace)
                && let Some(name) = head.split_whitespace().next()
                && !name.contains(['=', '$'])
            {
                self.scripts
                    .entry(name.into())
                    .or_insert_with(|| ScriptOrigin {
                        manifest: relative.into(),
                        kind: "just".into(),
                    });
            }
        }
        Ok(())
    }

    fn collect_cargo_aliases(&mut self, relative: &str, path: &Path) -> Result<()> {
        let contents = fs::read_to_string(path)?;
        let value: toml::Value =
            toml::from_str(&contents).with_context(|| format!("无法解析 {relative}"))?;
        if let Some(aliases) = value.get("alias").and_then(toml::Value::as_table) {
            for name in aliases.keys() {
                self.scripts
                    .entry(name.clone())
                    .or_insert_with(|| ScriptOrigin {
                        manifest: relative.into(),
                        kind: "cargo-alias".into(),
                    });
            }
        }
        Ok(())
    }

    fn collect_go_mod(&mut self, path: &Path) -> Result<()> {
        let contents = fs::read_to_string(path)?;
        let mut in_require = false;
        for line in contents.lines() {
            let trimmed = line.trim();
            if let Some(module) = trimmed.strip_prefix("module ") {
                self.go_module = Some(module.trim().to_owned());
            } else if trimmed == "require (" {
                in_require = true;
            } else if in_require && trimmed == ")" {
                in_require = false;
            } else if let Some(requirement) = trimmed.strip_prefix("require ") {
                if let Some(module) = requirement.split_whitespace().next() {
                    self.go_requires.push(module.into());
                }
            } else if in_require && !trimmed.starts_with("//") {
                if let Some(module) = trimmed.split_whitespace().next() {
                    self.go_requires.push(module.into());
                }
            }
        }
        Ok(())
    }

    fn collect_pyproject(&mut self, relative: &str, path: &Path) -> Result<()> {
        let contents = fs::read_to_string(path)?;
        let value: toml::Value =
            toml::from_str(&contents).with_context(|| format!("无法解析 {relative}"))?;
        if let Some(project) = value.get("project") {
            if let Some(name) = project.get("name").and_then(toml::Value::as_str) {
                self.packages.insert(name.to_owned());
            }
            if let Some(scripts) = project.get("scripts").and_then(toml::Value::as_table) {
                for name in scripts.keys() {
                    self.repo_bins.insert(name.clone(), relative.to_owned());
                }
            }
        }
        if let Some(tasks) = value
            .get("tool")
            .and_then(|tool| tool.get("poe"))
            .and_then(|poe| poe.get("tasks"))
            .and_then(toml::Value::as_table)
        {
            for name in tasks.keys() {
                self.scripts
                    .entry(name.clone())
                    .or_insert_with(|| ScriptOrigin {
                        manifest: relative.to_owned(),
                        kind: "poe".into(),
                    });
            }
        }
        Ok(())
    }

    fn base_path(&self, doc: &str, base: Base) -> PathBuf {
        match base {
            Base::RepoRoot => self.root.clone(),
            Base::DocDir => self
                .root
                .join(doc)
                .parent()
                .unwrap_or(&self.root)
                .to_path_buf(),
            Base::ProjectRoot => {
                let mut current = self
                    .root
                    .join(doc)
                    .parent()
                    .unwrap_or(&self.root)
                    .to_path_buf();
                loop {
                    if ["package.json", "Cargo.toml", "go.mod", "pyproject.toml"]
                        .iter()
                        .any(|manifest| current.join(manifest).is_file())
                    {
                        return current;
                    }
                    if ["AGENTS.md", "CLAUDE.md", "GEMINI.md"]
                        .iter()
                        .any(|instructions| current.join(instructions).is_file())
                    {
                        return current;
                    }
                    if current == self.root || !current.pop() {
                        return self.root.clone();
                    }
                }
            }
        }
    }
}

fn collect_files(root: &Path) -> Result<Vec<String>> {
    if root.join(".git").exists() {
        if let Ok(output) = Command::new("git")
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .current_dir(root)
            .output()
        {
            if output.status.success() {
                return Ok(output
                    .stdout
                    .split(|byte| *byte == 0)
                    .filter(|path| !path.is_empty())
                    .filter_map(|path| String::from_utf8(path.to_vec()).ok())
                    .filter(|path| !excluded_path(path))
                    .filter(|path| root.join(path).is_file())
                    .collect());
            }
        }
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !excluded_component(&entry.file_name().to_string_lossy()))
    {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Ok(relative) = entry.path().strip_prefix(root) {
                files.push(normalize(relative));
            }
        }
    }
    Ok(files)
}

fn collect_known_files(root: &Path) -> Result<Vec<String>> {
    if root.join(".git").exists()
        && let Ok(output) = Command::new("git")
            .args([
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ])
            .current_dir(root)
            .output()
        && output.status.success()
    {
        let deleted = Command::new("git")
            .args(["diff", "--name-only", "-z", "--diff-filter=D"])
            .current_dir(root)
            .output()
            .ok()
            .filter(|result| result.status.success())
            .map(|result| {
                result
                    .stdout
                    .split(|byte| *byte == 0)
                    .filter(|path| !path.is_empty())
                    .map(<[u8]>::to_vec)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        return Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .filter(|path| !deleted.contains(*path))
            .filter_map(|path| String::from_utf8(path.to_vec()).ok())
            .collect());
    }
    collect_files(root)
}

fn excluded_path(path: &str) -> bool {
    Path::new(path).components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(excluded_component)
    })
}

fn excluded_component(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".venv"
            | "node_modules"
            | "target"
            | ".next"
            | "dist"
            | "build"
            | "coverage"
            | "vendor"
    )
}

impl RepoFacts for FsRepoFacts {
    fn path_bases(&self, doc: &str) -> Vec<Base> {
        self.scopes
            .iter()
            .find_map(|(pattern, base)| glob_match(pattern, doc).then_some(vec![*base]))
            .unwrap_or_else(|| vec![Base::DocDir, Base::ProjectRoot, Base::RepoRoot])
    }

    fn file_exists(&self, doc: &str, base: Base, rel: &str) -> bool {
        self.resolve_path(doc, base, rel).is_some()
    }

    fn resolve_path(&self, doc: &str, base: Base, rel: &str) -> Option<String> {
        let path = self.base_path(doc, base).join(rel);
        let normalized = normalize_lexically(&path);
        let relative = normalized.strip_prefix(&self.root).ok().map(normalize)?;
        if !self.known_paths.contains(&relative) || !normalized.exists() {
            return None;
        }
        let canonical = normalized.canonicalize().ok()?;
        if !canonical.starts_with(&self.root) {
            return None;
        }
        normalized.strip_prefix(&self.root).ok().map(normalize)
    }

    fn glob_paths(&self, doc: &str, base: Base, pattern: &str) -> Vec<String> {
        let absolute_pattern = normalize_lexically(&self.base_path(doc, base).join(pattern));
        let Some(relative_pattern) = absolute_pattern
            .strip_prefix(&self.root)
            .ok()
            .map(normalize)
        else {
            return Vec::new();
        };
        let mut matches = self
            .known_paths
            .iter()
            .filter(|path| !path.is_empty() && glob_match(&relative_pattern, path))
            .filter_map(|path| self.resolve_path(doc, Base::RepoRoot, path))
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();
        matches
    }

    fn find_basename(&self, name: &str) -> Vec<String> {
        self.basenames.get(name).cloned().unwrap_or_default()
    }

    fn path_ignored(&self, rel: &str) -> bool {
        // 文档提到的路径命中 .gitignore，多半是运行时产物，只在这里问 git 一次并缓存。
        if !self.is_git {
            return false;
        }
        let key = rel.trim_end_matches('/').to_owned();
        if key.is_empty() {
            return false;
        }
        if let Some(cached) = self
            .ignore_hits
            .lock()
            .expect("ignore cache is not poisoned")
            .get(&key)
        {
            return *cached;
        }
        let ignored = Command::new("git")
            .args(["check-ignore", "-q", "--", &key])
            .current_dir(&self.root)
            .output()
            .is_ok_and(|output| output.status.success());
        self.ignore_hits
            .lock()
            .expect("ignore cache is not poisoned")
            .insert(key, ignored);
        ignored
    }

    fn script(&self, name: &str) -> Option<ScriptOrigin> {
        self.scripts.get(name).cloned()
    }

    fn script_names(&self) -> Vec<String> {
        self.scripts.keys().cloned().collect()
    }

    fn workspace_pkg(&self, name: &str) -> bool {
        self.packages.contains(name)
    }

    fn workspace_packages(&self) -> Vec<String> {
        self.packages.iter().cloned().collect()
    }

    fn binary_known(&self, name: &str) -> BinKnowledge {
        if let Some(origin) = self.repo_bins.get(name) {
            return BinKnowledge::Repo {
                origin: origin.clone(),
            };
        }
        if tool_table().contains_key(name) {
            return BinKnowledge::ToolTable;
        }
        if executable_on_path(name) {
            return BinKnowledge::Path;
        }
        BinKnowledge::Unknown
    }

    fn tool_subcommand_known(&self, tool: &str, subcommand: &str) -> bool {
        tool_table()
            .get(tool)
            .is_some_and(|spec| spec.subcommands.contains(subcommand))
    }

    fn tool_subcommand_replacement(&self, tool: &str, subcommand: &str) -> Option<String> {
        tool_table()
            .get(tool)
            .and_then(|spec| spec.renamed.get(subcommand))
            .cloned()
    }

    fn has_go_mod(&self) -> bool {
        self.go_module.is_some()
    }

    fn go_import_known(&self, import: &str) -> bool {
        const STDLIB_HEADS: &[&str] = &[
            "archive",
            "bufio",
            "bytes",
            "cmp",
            "compress",
            "container",
            "context",
            "crypto",
            "database",
            "debug",
            "embed",
            "encoding",
            "errors",
            "expvar",
            "flag",
            "fmt",
            "go",
            "hash",
            "html",
            "image",
            "index",
            "io",
            "log",
            "maps",
            "math",
            "mime",
            "net",
            "os",
            "path",
            "plugin",
            "reflect",
            "regexp",
            "runtime",
            "slices",
            "sort",
            "strconv",
            "strings",
            "sync",
            "syscall",
            "testing",
            "text",
            "time",
            "unicode",
            "unsafe",
        ];
        let head = import.split('/').next().unwrap_or(import);
        STDLIB_HEADS.contains(&head)
            || self
                .go_module
                .as_ref()
                .is_some_and(|module| import == module || import.starts_with(&format!("{module}/")))
            || self
                .go_requires
                .iter()
                .any(|module| import == module || import.starts_with(&format!("{module}/")))
    }

    fn grep_word(&self, word: &str) -> Option<FirstHit> {
        let index = self
            .word_index
            .get_or_init(|| build_word_index(&self.root, &self.files));
        index.get(word).map(|&(file, line)| FirstHit {
            path: self.files[file as usize].clone(),
            line: line as usize,
        })
    }

    fn config_key(&self, doc: &str, file_hint: Option<&str>, key: &str) -> Option<FirstHit> {
        let doc_dir = Path::new(doc).parent().unwrap_or(Path::new(""));
        let project_dir = self
            .base_path(doc, Base::ProjectRoot)
            .strip_prefix(&self.root)
            .ok()
            .map(normalize)
            .unwrap_or_default();
        let mut candidates: Vec<&String> = self
            .files
            .iter()
            .filter(|path| config_file(path))
            .filter(|path| {
                if let Some(hint) = file_hint {
                    path.as_str() == hint
                        || path.ends_with(&format!("/{hint}"))
                        || Path::new(path).file_name() == Path::new(hint).file_name()
                } else {
                    let parent = Path::new(path).parent().unwrap_or(Path::new(""));
                    parent == doc_dir || normalize(parent) == project_dir
                }
            })
            .collect();
        candidates.sort_by_key(|path| usize::from(!Path::new(path).starts_with(doc_dir)));
        for relative in candidates {
            let Ok(contents) = fs::read_to_string(self.root.join(relative)) else {
                continue;
            };
            for (index, line) in contents.lines().enumerate() {
                let trimmed = line.trim_start().trim_start_matches(['"', '\'']);
                if trimmed.strip_prefix(key).is_some_and(|remainder| {
                    remainder.trim_start().starts_with([':', '=', '"', '\''])
                }) {
                    return Some(FirstHit {
                        path: relative.clone(),
                        line: index + 1,
                    });
                }
            }
        }
        None
    }

    fn content_hash(&self, doc: &str, base: Base, rel: &str) -> Option<String> {
        let path = self.resolve_path(doc, base, rel)?;
        let bytes = fs::read(self.root.join(path)).ok()?;
        Some(hex::encode(Sha256::digest(bytes)))
    }
}

fn normalize(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            Component::ParentDir => Some("..".into()),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                output.pop();
            }
            Component::CurDir => {}
            _ => output.push(component.as_os_str()),
        }
    }
    output
}

fn searchable(path: &str) -> bool {
    !path.ends_with(".md")
        && Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension,
                    "rs" | "js"
                        | "jsx"
                        | "ts"
                        | "tsx"
                        | "py"
                        | "go"
                        | "java"
                        | "kt"
                        | "swift"
                        | "c"
                        | "h"
                        | "cpp"
                        | "rb"
                        | "php"
                        | "sh"
                        | "toml"
                        | "yaml"
                        | "yml"
                        | "json"
                )
            })
}

fn config_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "toml" | "yaml" | "yml" | "json"))
}

/// 对可搜索文件做一遍扫描，记下每个词的首个命中位置。
/// 词的切分规则和上限（长度 3..=128、单文件 1 MiB）与逐词扫描时代一致。
fn build_word_index(root: &Path, files: &[String]) -> HashMap<String, (u32, u32)> {
    let mut index = HashMap::new();
    for (file_index, relative) in files.iter().enumerate() {
        if !searchable(relative) {
            continue;
        }
        let path = root.join(relative);
        if path
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 1_048_576)
        {
            continue;
        }
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        for (line_index, line) in contents.lines().enumerate() {
            for word in line_words(line) {
                if word.len() > 128 {
                    continue;
                }
                index
                    .entry(word.to_owned())
                    .or_insert((file_index as u32, line_index as u32 + 1));
            }
        }
    }
    index
}

fn line_words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|character: char| {
        character != '_'
            && character != '-'
            && character != ':'
            && !character.is_ascii_alphanumeric()
    })
    .flat_map(|token| {
        let token = token.trim_matches(['-', ':']);
        std::iter::once(token).chain(token.split(':'))
    })
    .filter(|token| token.len() > 2)
}

fn executable_on_path(name: &str) -> bool {
    if name.contains('/') {
        return false;
    }
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| {
            let path = directory.join(name);
            path.is_file()
        })
    })
}

#[derive(Debug, Deserialize)]
struct ToolSpec {
    subcommands: HashSet<String>,
    #[serde(default)]
    renamed: HashMap<String, String>,
}

fn tool_table() -> &'static HashMap<String, ToolSpec> {
    static TABLE: OnceLock<HashMap<String, ToolSpec>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!("../data/tools.json"))
            .expect("vendored tools.json must be valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_file_tree_excludes_ignored_paths_and_word_search_is_repeatable() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("src")).unwrap();
        fs::create_dir_all(directory.path().join("config")).unwrap();
        fs::create_dir_all(directory.path().join("crates/library/src")).unwrap();
        fs::create_dir_all(directory.path().join("crates/tool/src/bin")).unwrap();
        fs::create_dir_all(directory.path().join("vendor/library")).unwrap();
        fs::write(directory.path().join(".gitignore"), "generated/\n").unwrap();
        fs::write(directory.path().join("AGENTS.md"), "See `src/main.rs`.\n").unwrap();
        fs::write(
            directory.path().join("src/main.rs"),
            "fn stable_symbol() {}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("config/settings.toml"),
            "mode = 'safe'\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("crates/library/Cargo.toml"),
            "[package]\nname = 'attest-fixture-library-only'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("crates/library/src/lib.rs"),
            "pub fn library_only() {}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("crates/tool/Cargo.toml"),
            "[package]\nname = 'attest-fixture-tool'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("crates/tool/src/main.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("crates/tool/src/bin/helper.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("vendor/library/source.c"),
            "int vendored(void) { return 1; }\n",
        )
        .unwrap();
        fs::write(directory.path().join("deleted.txt"), "gone\n").unwrap();
        for args in [
            &["init"][..],
            &["config", "user.email", "attest@example.invalid"],
            &["config", "user.name", "attest test"],
            &["add", "."],
            &["commit", "-m", "initial"],
        ] {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(directory.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        fs::create_dir_all(directory.path().join("generated")).unwrap();
        fs::write(directory.path().join("generated/cache.json"), "{}\n").unwrap();
        fs::remove_file(directory.path().join("deleted.txt")).unwrap();

        let facts = FsRepoFacts::collect(directory.path(), &[]).unwrap();
        assert!(facts.file_exists("AGENTS.md", Base::RepoRoot, "src/main.rs"));
        assert!(facts.file_exists("AGENTS.md", Base::RepoRoot, "src/"));
        assert_eq!(
            facts.glob_paths("AGENTS.md", Base::RepoRoot, "src/*.rs"),
            ["src/main.rs"]
        );
        assert!(
            facts
                .glob_paths("AGENTS.md", Base::RepoRoot, "missing/*.rs")
                .is_empty()
        );
        assert!(!facts.file_exists("AGENTS.md", Base::RepoRoot, "generated/cache.json"));
        assert!(!facts.file_exists("AGENTS.md", Base::RepoRoot, "deleted.txt"));
        assert!(facts.file_exists("AGENTS.md", Base::RepoRoot, "vendor/"));
        assert!(facts.file_exists("AGENTS.md", Base::RepoRoot, "vendor/library/source.c"));
        assert!(
            facts
                .files()
                .iter()
                .all(|path| !path.starts_with("vendor/"))
        );
        assert!(
            facts
                .find_basename("library")
                .contains(&"vendor/library".into())
        );
        assert_eq!(
            facts.binary_known("attest-fixture-library-only"),
            BinKnowledge::Unknown
        );
        assert!(matches!(
            facts.binary_known("attest-fixture-tool"),
            BinKnowledge::Repo { .. }
        ));
        assert!(matches!(
            facts.binary_known("helper"),
            BinKnowledge::Repo { .. }
        ));
        let expected = Some(FirstHit {
            path: "src/main.rs".into(),
            line: 1,
        });
        assert_eq!(facts.grep_word("stable_symbol"), expected);
        assert_eq!(facts.grep_word("stable_symbol"), expected);
        assert_eq!(facts.config_key("AGENTS.md", None, "mode"), None);
        assert_eq!(
            facts.config_key("AGENTS.md", Some("config/settings.toml"), "mode"),
            Some(FirstHit {
                path: "config/settings.toml".into(),
                line: 1,
            })
        );
    }

    #[test]
    fn manifestless_nested_agent_project_uses_instruction_root() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("demo/.claude/agents")).unwrap();
        fs::create_dir_all(directory.path().join("demo/financial_data")).unwrap();
        fs::write(
            directory.path().join("pyproject.toml"),
            "[project]\nname = 'outer'\nversion = '0.1.0'\n",
        )
        .unwrap();
        fs::write(directory.path().join("demo/CLAUDE.md"), "# Demo\n").unwrap();
        fs::write(
            directory.path().join("demo/.claude/agents/analyst.md"),
            "Read `financial_data/`.\n",
        )
        .unwrap();
        fs::write(directory.path().join("demo/financial_data/data.csv"), "x\n").unwrap();

        let facts = FsRepoFacts::collect(directory.path(), &[]).unwrap();
        assert_eq!(
            facts.resolve_path(
                "demo/.claude/agents/analyst.md",
                Base::ProjectRoot,
                "financial_data"
            ),
            Some("demo/financial_data".into())
        );
    }
}
