//! Replayable filename selection policy. Bindings execute actions, never choose fallbacks.
use crate::parser_routing::{filename_candidates, shortlist, Candidate, RoutingQuery};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize)]
pub struct Request {
    pub entries: Vec<Candidate>,
    pub filename: String,
    #[serde(default)] pub content: String,
    pub language_hint: Option<String>,
    pub plugin_id: Option<String>,
    #[serde(default)] pub strict: bool,
    pub allowed_plugins: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Loaded { index: usize, grammar_id: String, languages: Vec<String>, priority: i32 },
    LoadFailed { index: usize, terminal: bool },
    Probed { index: usize, language: Option<String> },
    ProbeFailed { index: usize },
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Load { index: usize },
    Probe { index: usize, sample: String },
    Selected { index: usize, language: String },
    NotFound { identity: String },
    /// Re-raise the original host exception associated with this event.
    Failure { event: usize },
}

fn terminal(action: Action, cursor: usize, events: &[Event]) -> Result<Action, String> {
    if cursor != events.len() { return Err("unsolicited filename selection events after outcome".into()); }
    Ok(action)
}

/// Metadata of every candidate in the current phase is loaded before priority ranking.
/// A fuel/trust failure during this discovery is terminal, even if another candidate
/// might work. Unrelated later phases are never loaded after a successful selection.
/// Non-strict hints fall through to specific filename candidates, other specific
/// candidates, then generic. Explicit plugin requests cannot cross this boundary.
pub fn next_action(request: &Request, events: &[Event]) -> Result<Action, String> {
    if events.len() > request.entries.len().saturating_mul(2) {
        return Err("too many filename selection events".into());
    }
    let base = RoutingQuery { filename: request.filename.clone(), ..Default::default() };
    // Also validates nonempty, unique catalogue identities.
    let all = shortlist(&request.entries, &RoutingQuery::default())?;
    let generic = |i: &usize| request.entries[*i].languages.iter().any(|l| l == "generic");
    let mut phases: Vec<(bool, Vec<usize>)> = Vec::new();
    if let Some(plugin) = &request.plugin_id {
        phases.push((request.language_hint.is_some(), shortlist(&request.entries,
            &RoutingQuery { plugin_id: Some(plugin.clone()), ..base })?));
    } else {
        if let Some(hint) = &request.language_hint {
            phases.push((true, shortlist(&request.entries, &RoutingQuery {
                language_hint: Some(hint.clone()), ..Default::default()
            })?));
        }
        let primary = filename_candidates(&request.entries, &request.filename)?;
        phases.push((false, primary.iter().copied().filter(|i| !generic(i)).collect()));
        phases.push((false, all.iter().copied().filter(|i| !primary.contains(i) && !generic(i)).collect()));
        phases.push((false, all.iter().copied().filter(generic).collect()));
    }
    let mut cursor = 0;
    let mut loaded: BTreeMap<usize, (&str, &[String], i32)> = BTreeMap::new();
    let mut failed = BTreeSet::new();
    let mut probed = BTreeSet::new();
    let mut deferred_generic = BTreeSet::new();
    let mut end = request.content.len().min(2048);
    while !request.content.is_char_boundary(end) { end -= 1; }
    let sample = &request.content[..end];
    let last_phase = phases.len().saturating_sub(1);
    for (phase, (hint_phase, mut indices)) in phases.into_iter().enumerate() {
        if phase == last_phase && request.plugin_id.is_none() {
            indices.extend(deferred_generic.iter().copied());
            indices.sort_unstable(); indices.dedup();
        }
        for &index in &indices {
            if loaded.contains_key(&index) || failed.contains(&index) { continue; }
            let Some(event) = events.get(cursor) else { return Ok(Action::Load { index }); };
            match event {
                Event::Loaded { index: observed, grammar_id, languages, priority } if *observed == index => {
                    if grammar_id.is_empty() || languages.iter().any(|s| s.is_empty()) {
                        return Err("loaded parser has empty grammar/language identity".into());
                    }
                    loaded.insert(index, (grammar_id, languages, *priority));
                }
                Event::LoadFailed { index: observed, terminal: is_terminal } if *observed == index => {
                    if *is_terminal { return terminal(Action::Failure { event: cursor }, cursor + 1, events); }
                    failed.insert(index);
                }
                _ => return Err("out-of-order filename selection load event".into()),
            }
            cursor += 1;
        }
        indices.retain(|i| loaded.contains_key(i));
        indices.sort_by(|a,b| {
            let (_,al,ap) = loaded[a]; let (_,bl,bp) = loaded[b];
            al.iter().any(|l| l == "generic").cmp(&bl.iter().any(|l| l == "generic"))
                .then(bp.cmp(&ap)).then(request.entries[*a].id.cmp(&request.entries[*b].id))
        });
        for index in indices {
            let (grammar, languages, _) = loaded[&index];
            if request.allowed_plugins.as_ref().is_some_and(|allow| !allow.iter().any(|id| id == grammar)) { continue; }
            if hint_phase {
                let hint = request.language_hint.as_ref().expect("hint phase");
                let declared = languages.contains(hint);
                let catalog_alias = request.plugin_id.is_none() && languages.iter().any(|l| l == "generic")
                    && request.entries[index].languages.contains(hint);
                if declared || catalog_alias {
                    return terminal(Action::Selected { index, language: hint.clone() }, cursor, events);
                }
                continue;
            }
            if request.plugin_id.is_none() && phase != last_phase && languages.iter().any(|l| l == "generic") {
                deferred_generic.insert(index); continue;
            }
            if !probed.insert(index) { continue; }
            let Some(event) = events.get(cursor) else { return Ok(Action::Probe { index, sample: sample.into() }); };
            match event {
                Event::Probed { index: observed, language } if *observed == index => {
                    if let Some(language) = language.as_deref().filter(|s| !s.is_empty()) {
                        if !languages.iter().any(|id| id == language) {
                            return Err(format!("parser {} claimed undeclared language {language}", request.entries[index].id));
                        }
                        return terminal(Action::Selected { index, language: language.into() }, cursor + 1, events);
                    }
                }
                Event::ProbeFailed { index: observed } if *observed == index => {
                    return terminal(Action::Failure { event: cursor }, cursor + 1, events);
                }
                _ => return Err("out-of-order filename selection probe event".into()),
            }
            cursor += 1;
        }
        if hint_phase && request.strict && request.plugin_id.is_none() {
            return terminal(Action::NotFound { identity: request.language_hint.clone().unwrap() }, cursor, events);
        }
    }
    terminal(Action::NotFound { identity: request.plugin_id.clone().unwrap_or_else(|| "unknown".into()) }, cursor, events)
}

pub(crate) fn next_action_json_impl(input: &str) -> Result<String, String> {
    #[derive(Deserialize)] struct Input { request: Request, events: Vec<Event> }
    let input: Input = serde_json::from_str(input).map_err(|e| e.to_string())?;
    serde_json::to_string(&next_action(&input.request, &input.events)?).map_err(|e| e.to_string())
}
