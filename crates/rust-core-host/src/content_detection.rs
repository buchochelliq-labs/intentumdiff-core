//! Content-probe policy shared by native callers and language bindings.
//! Hosts load components and execute the planned probes; Rust owns all decisions.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Candidate {
    pub plugin_id: String,
    pub grammar_id: String,
    pub languages: Vec<String>,
    pub priority: i32,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Request {
    pub entries: Vec<Candidate>,
    #[serde(default)] pub content: String,
    pub allowed_plugins: Option<Vec<String>>,
    #[serde(default)] pub candidates: Vec<String>,
    pub plugin_id: Option<String>,
    #[serde(default)] pub preferred_plugins: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    pub indices: Vec<usize>,
    pub sample: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Observation {
    pub index: usize,
    pub language: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Detection {
    pub language: String,
    pub grammar_id: String,
    /// Reciprocal rank rounded to three decimals (ties to even), not probability.
    pub confidence: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Outcome {
    pub results: Vec<Detection>,
    pub not_found: Option<String>,
}

/// Validate loaded metadata, select eligible probes, and bound input to 4096 UTF-8 bytes.
/// An empty allowlist excludes all plugins; an empty language filter is unrestricted.
pub fn plan(request: &Request) -> Result<Plan, String> {
    let mut seen = BTreeSet::new();
    for entry in &request.entries {
        if entry.plugin_id.is_empty() || !seen.insert(&entry.plugin_id) {
            return Err("content parser plugin identities must be nonempty and unique".into());
        }
        if entry.grammar_id.is_empty() || entry.languages.iter().any(|s| s.is_empty()) {
            return Err("content parser metadata has an empty grammar or language identity".into());
        }
    }
    let mut indices: Vec<_> = request.entries.iter().enumerate().filter_map(|(index, entry)| {
        let allowed = request.allowed_plugins.as_ref().is_none_or(|ids| ids.contains(&entry.grammar_id));
        let selected = request.plugin_id.as_ref().is_none_or(|id| id == &entry.plugin_id);
        let language = request.candidates.is_empty() || entry.languages.iter().any(|id| request.candidates.contains(id));
        (allowed && selected && language).then_some(index)
    }).collect();
    indices.sort_by(|&a, &b| request.entries[b].priority.cmp(&request.entries[a].priority)
        .then(request.entries[a].plugin_id.cmp(&request.entries[b].plugin_id)));
    let mut end = request.content.len().min(4096);
    while !request.content.is_char_boundary(end) { end -= 1; }
    Ok(Plan { indices, sample: request.content[..end].to_owned() })
}

/// Every planned probe must supply exactly one observation, including declines.
/// Unsupported claims are contract errors. Host/probe errors must never be sent as declines.
/// Multiple plugins claiming one language remain separate results. Generic is always last.
pub fn finish(request: &Request, observations: &[Observation]) -> Result<Outcome, String> {
    let plan = plan(request)?;
    let expected: BTreeSet<_> = plan.indices.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut matches = Vec::new();
    for observation in observations {
        if !expected.contains(&observation.index) || !seen.insert(observation.index) {
            return Err("content probe observations contain an ineligible or duplicate index".into());
        }
        let entry = &request.entries[observation.index];
        if let Some(language) = observation.language.as_deref().filter(|s| !s.is_empty()) {
            if !entry.languages.iter().any(|s| s == language) {
                return Err(format!("parser {} claimed undeclared language {language}", entry.plugin_id));
            }
            if !request.candidates.is_empty() && !request.candidates.iter().any(|s| s == language) {
                return Err(format!("parser {} claimed language outside requested candidates: {language}", entry.plugin_id));
            }
            matches.push((entry, language));
        }
    }
    if seen != expected { return Err("content probe observations are incomplete".into()); }
    matches.sort_by(|(a, al), (b, bl)| {
        let preferred = |entry: &Candidate, lang: &str| request.preferred_plugins.get(lang) == Some(&entry.plugin_id);
        (*al == "generic").cmp(&(*bl == "generic"))
            .then(preferred(b, bl).cmp(&preferred(a, al)))
            .then(b.priority.cmp(&a.priority)).then(al.cmp(bl))
            .then(a.grammar_id.cmp(&b.grammar_id)).then(a.plugin_id.cmp(&b.plugin_id))
    });
    let not_found = if matches.is_empty() { request.plugin_id.clone() } else { None };
    let results = matches.into_iter().enumerate().map(|(rank, (entry, language))| Detection {
        language: language.into(), grammar_id: entry.grammar_id.clone(),
        confidence: (1000.0 / (rank + 1) as f64).round_ties_even() / 1000.0,
    }).collect();
    Ok(Outcome { results, not_found })
}

pub(crate) fn plan_json_impl(input: &str) -> Result<String, String> {
    let request = serde_json::from_str(input).map_err(|e| e.to_string())?;
    serde_json::to_string(&plan(&request)?).map_err(|e| e.to_string())
}
pub(crate) fn finish_json_impl(input: &str) -> Result<String, String> {
    #[derive(Deserialize)] struct Input { request: Request, observations: Vec<Observation> }
    let input: Input = serde_json::from_str(input).map_err(|e| e.to_string())?;
    serde_json::to_string(&finish(&input.request, &input.observations)?).map_err(|e| e.to_string())
}
