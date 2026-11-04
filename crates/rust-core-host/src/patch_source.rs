//! Unified patch reconstruction shared by native callers and all bindings.
use diffy::{HunkRange, Line};
use diffy::patch_set::{FileOperation, ParseOptions, PatchSet};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconstructionScope { Complete, Excerpt }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchContent {
    pub old_content: String,
    pub new_content: String,
    pub filename: String,
    pub scope: ReconstructionScope,
    pub warning: Option<String>,
}

const EXCERPT_WARNING: &str = "Patch-only reconstruction is an excerpt; unchanged trailing content is unknown. Provide the original content for a complete file review.";

fn start(range: HunkRange) -> Result<usize, String> {
    if range.is_empty() { Ok(range.start()) }
    else { range.start().checked_sub(1).ok_or("patch nonempty range starts at zero".into()) }
}

/// Apply one text-file patch at its declared positions, checking every old/context line.
/// Without an original, contiguous visible content is explicitly an excerpt (except
/// file creation/deletion). Unknown prefix/internal gaps require an original.
pub fn reconstruct(patch_text: &str, original: Option<&str>, filename: Option<&str>, require_complete: bool) -> Result<PatchContent, String> {
    if patch_text.trim().is_empty() {
        if require_complete && original.is_none() {
            return Err("original content required for a complete file review".into());
        }
        let scope = if original.is_some() { ReconstructionScope::Complete } else { ReconstructionScope::Excerpt };
        let warning = original.is_none().then(|| EXCERPT_WARNING.to_owned());
        let content = original.unwrap_or("").to_owned();
        return Ok(PatchContent {old_content:content.clone(),new_content:content,filename:filename.unwrap_or("unknown").to_owned(),scope,warning});
    }
    let opts = if patch_text.lines().any(|line| line.starts_with("diff --git ")) { ParseOptions::gitdiff() } else { ParseOptions::unidiff() };
    let files = PatchSet::parse(patch_text, opts).collect::<Result<Vec<_>, _>>().map_err(|e|format!("invalid patch: {e}"))?;
    if files.len() != 1 { return Err("patch reconstruction requires a single-file text patch".into()); }
    let file = &files[0];
    let patch = file.patch().as_text().ok_or("binary patch reconstruction is unsupported")?;
    let operation = file.operation();
    let path = match operation {
        FileOperation::Create(path) | FileOperation::Delete(path) => path.as_ref(),
        FileOperation::Modify { modified, .. } => modified.as_ref(),
        FileOperation::Rename { to, .. } | FileOperation::Copy { to, .. } => to.as_ref(),
    };
    let prefixed = match operation {
        FileOperation::Modify { original, modified } => original.starts_with("a/") && modified.starts_with("b/"),
        FileOperation::Create(path) => path.starts_with("b/"),
        FileOperation::Delete(path) => path.starts_with("a/"),
        _ => patch_text.lines().any(|line| line.starts_with("diff --git ")),
    };
    let path = if prefixed { path.strip_prefix("b/").or_else(||path.strip_prefix("a/")).unwrap_or(path) } else { path };
    let filename = filename.filter(|name| !name.is_empty()).unwrap_or(path).to_owned();
    if operation.is_create() && original.is_some_and(|source| !source.is_empty()) {
        return Err("creation patch requires empty original content".into());
    }
    let old_content = if let Some(source) = original { source.to_owned() } else {
        if patch.hunks().is_empty() && !operation.is_create() && !operation.is_delete() {
            return Err("original content required for a patch without text hunks".into());
        }
        let mut reconstructed = String::new();
        let mut count = 0;
        for hunk in patch.hunks() {
            if start(hunk.old_range())? != count { return Err("original content required: patch omits a prefix or internal gap".into()); }
            for line in hunk.lines() {
                if let Line::Context(text) | Line::Delete(text) = line { reconstructed.push_str(text); count += 1; }
            }
        }
        reconstructed
    };
    let scope = if original.is_some() || operation.is_create() || operation.is_delete() {
        ReconstructionScope::Complete
    } else { ReconstructionScope::Excerpt };
    if require_complete && scope == ReconstructionScope::Excerpt {
        return Err("original content required for a complete file review".into());
    }
    let old_lines: Vec<&str> = old_content.split_inclusive('\n').collect();
    let mut new_lines = Vec::new();
    let mut cursor = 0;
    for hunk in patch.hunks() {
        let offset = start(hunk.old_range())?;
        if offset < cursor { return Err("patch hunks overlap or are out of order".into()); }
        if offset > old_lines.len() { return Err("patch hunk is outside original content".into()); }
        new_lines.extend_from_slice(&old_lines[cursor..offset]);
        cursor = offset;
        if start(hunk.new_range())? != new_lines.len() { return Err("patch target range is inconsistent with preceding hunks".into()); }
        for line in hunk.lines() {
            match line {
                Line::Context(text) | Line::Delete(text) => {
                    if old_lines.get(cursor).copied() != Some(*text) {
                        return Err(format!("patch context/removal mismatch at original line {}",cursor+1));
                    }
                    cursor += 1;
                    if matches!(line, Line::Context(_)) { new_lines.push(*text); }
                },
                Line::Insert(text) => new_lines.push(*text),
            }
        }
    }
    new_lines.extend_from_slice(&old_lines[cursor..]);
    if new_lines.iter().take(new_lines.len().saturating_sub(1)).any(|line| !line.ends_with('\n')) {
        return Err("patch appends content after a line without a final newline".into());
    }
    let new_content = new_lines.concat();
    if operation.is_delete() && !new_content.is_empty() { return Err("deletion patch leaves original content behind".into()); }
    let warning = (scope == ReconstructionScope::Excerpt).then(|| EXCERPT_WARNING.to_owned());
    Ok(PatchContent { old_content, new_content, filename, scope, warning })
}

#[derive(Deserialize)]
struct Request {
    patch_text: String,
    original_content: Option<String>,
    filename: Option<String>,
    #[serde(default)]
    require_complete: bool,
}

pub(crate) fn reconstruct_json_impl(request: &str) -> Result<String, String> {
    let request: Request = serde_json::from_str(request).map_err(|e| e.to_string())?;
    let result = reconstruct(&request.patch_text, request.original_content.as_deref(), request.filename.as_deref(), request.require_complete)?;
    serde_json::to_string(&result).map_err(|e| e.to_string())
}
