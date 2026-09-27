//! Shared deterministic parser candidate planning. Hosts supply discovered metadata.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct Candidate {
    /// Stable unique host identity (for example the resolved component path).
    pub id: String,
    #[serde(default)] pub aliases: Vec<String>,
    #[serde(default)] pub languages: Vec<String>,
    #[serde(default)] pub filenames: Vec<String>,
    #[serde(default)] pub extensions: Vec<String>,
    #[serde(default)] pub priority: i32,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct RoutingQuery {
    #[serde(default)] pub filename: String,
    pub language_hint: Option<String>,
    pub plugin_id: Option<String>,
    #[serde(default)] pub candidates: Vec<String>,
}

fn matches_filename(candidate: &Candidate, filename: &str) -> bool {
    if filename.is_empty() { return false; }
    let normalized = filename.replace('\\', "/").to_lowercase();
    let name = normalized.rsplit('/').next().unwrap_or("");
    candidate.filenames.iter().any(|value| name == value.to_lowercase()) ||
        candidate.extensions.iter().any(|extension| {
            let extension = extension.to_lowercase();
            if extension.starts_with('.') { name.ends_with(&extension) }
            else { name == extension }
        })
}

fn ordered(candidates: &[Candidate], mut indices: Vec<usize>) -> Vec<usize> {
    indices.sort_by(|&left, &right| {
        let a = &candidates[left]; let b = &candidates[right];
        let generic = |candidate: &Candidate| candidate.languages.iter().any(|language| language == "generic");
        generic(a).cmp(&generic(b)).then(b.priority.cmp(&a.priority)).then(a.id.cmp(&b.id))
    });
    indices
}

fn validate(candidates: &[Candidate]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        if candidate.id.is_empty() || !seen.insert(&candidate.id) {
            return Err("parser candidate identities must be nonempty and unique".into());
        }
    }
    Ok(())
}

/// Return matching filename candidates only (no implicit generic fallback).
pub fn filename_candidates(candidates: &[Candidate], filename: &str) -> Result<Vec<usize>, String> {
    validate(candidates)?;
    Ok(ordered(candidates, candidates.iter().enumerate().filter_map(|(i,c)| matches_filename(c, filename).then_some(i)).collect()))
}

/// Explicit plugin aliases take precedence, then language filters, then filename.
/// Without a filename match, all candidates remain eligible, with generic last.
/// Results are source indices, ordered by generic status, descending declared
/// priority and stable identity. This does not infer successful component loading.
pub fn shortlist(candidates: &[Candidate], query: &RoutingQuery) -> Result<Vec<usize>, String> {
    validate(candidates)?;
    let mut indices: Vec<usize> = if let Some(plugin) = &query.plugin_id {
        candidates.iter().enumerate().filter_map(|(i,c)| (c.id == *plugin || c.aliases.contains(plugin)).then_some(i)).collect()
    } else if query.language_hint.is_some() || !query.candidates.is_empty() {
        candidates.iter().enumerate().filter_map(|(i,c)| c.languages.iter().any(|language| query.language_hint.as_ref() == Some(language) || query.candidates.contains(language)).then_some(i)).collect()
    } else {
        filename_candidates(candidates, &query.filename)?
    };
    if indices.is_empty() && query.plugin_id.is_none() && query.language_hint.is_none() && query.candidates.is_empty() {
        indices.extend(0..candidates.len());
    }
    Ok(ordered(candidates, indices))
}

#[derive(Deserialize)]
struct Request { entries: Vec<Candidate>, query: RoutingQuery }
pub(crate) fn shortlist_json_impl(request: &str) -> Result<String, String> {
    let request: Request = serde_json::from_str(request).map_err(|e|e.to_string())?;
    serde_json::to_string(&shortlist(&request.entries, &request.query)?).map_err(|e|e.to_string())
}
