//! Pure, repository-relative ignore matching. Hosts supply all rule-file contents.
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct IgnoreFile {
    /// Repository-relative POSIX directory, or empty for the root.
    pub directory: String,
    pub content: String,
}

/// Compiled rules with per-directory precedence and excluded-parent pruning.
pub struct IgnoreRules { files: Vec<(String, Gitignore)> }

fn validate_path(path: &str, allow_root: bool) -> Result<(), String> {
    if allow_root && path.is_empty() { return Ok(()); }
    if path.is_empty() || path.contains('\\') || path.contains('\0') || path.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return Err("expected a normalized repository-relative POSIX path".into());
    }
    Ok(())
}

impl IgnoreRules {
    pub fn new(files: &[IgnoreFile]) -> Result<Self, String> {
        let mut files = files.to_vec();
        for file in &files { validate_path(&file.directory, true)?; }
        files.sort_by(|a,b| a.directory.split('/').count().cmp(&b.directory.split('/').count()).then(a.directory.cmp(&b.directory)));
        let mut compiled = Vec::new();
        for file in files {
            if compiled.iter().any(|(directory, _)| directory == &file.directory) { return Err("duplicate ignore directory".into()); }
            let mut builder = GitignoreBuilder::new("");
            for line in file.content.lines() { builder.add_line(None, line).map_err(|e| e.to_string())?; }
            compiled.push((file.directory, builder.build().map_err(|e|e.to_string())?));
        }
        Ok(Self { files: compiled })
    }

    /// Paths use forward slashes; matching is case-sensitive on every platform.
    pub fn is_ignored(&self, path: &str, is_dir: bool) -> Result<bool, String> {
        validate_path(path, false)?;
        let components: Vec<&str> = path.split('/').collect();
        // Git cannot re-include a child while its parent remains excluded.
        for count in 1..=components.len() {
            let candidate = components[..count].join("/");
            let directory = count < components.len() || is_dir;
            let mut ignored = false;
            for (root, rules) in &self.files {
                let relative = if root.is_empty() { candidate.as_str() }
                    else if let Some(relative) = candidate.strip_prefix(&format!("{root}/")) { relative }
                    else { continue };
                let matched = rules.matched(relative, directory);
                if !matched.is_none() { ignored = matched.is_ignore(); }
            }
            if ignored { return Ok(true); }
        }
        Ok(false)
    }
}

#[derive(Deserialize)]
struct Query { path: String, #[serde(default)] is_dir: bool }
#[derive(Deserialize)]
struct Request { files: Vec<IgnoreFile>, paths: Vec<Query> }
pub(crate) fn match_json_impl(request: &str) -> Result<String, String> {
    let request: Request = serde_json::from_str(request).map_err(|e|e.to_string())?;
    let rules = IgnoreRules::new(&request.files)?;
    let matches = request.paths.iter().map(|query| rules.is_ignored(&query.path, query.is_dir)).collect::<Result<Vec<_>,_>>()?;
    serde_json::to_string(&matches).map_err(|e|e.to_string())
}
