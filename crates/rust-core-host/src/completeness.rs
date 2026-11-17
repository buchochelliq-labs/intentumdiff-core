//! `SemanticDiff.completeness` — whether a served review fully explains its source delta
//! (core #252).
//!
//! One classification for every entry point (C ABI ops, routed review, live diff, CLI,
//! batch/commit). Every served diff passes `crate::apply_file_lifecycle_to_diff`, which calls
//! [`attach`]; the routed review attaches it with sources before that.
//!
//! * `complete`   — the `changes` are the review: there is at least one change, or the two
//!   sources are byte-identical (nothing to explain).
//! * `equivalent` — the sources differ and the engine PROVED they are equivalent: style-only
//!   (`is_style_only`) or a suppressor that recorded an explicit `metadata.equivalence_proof`.
//! * `incomplete` — the sources differ, no change survived, and nothing proved equivalence.
//!   An empty change list is not an equivalence proof, so this is never a clean review. The
//!   DTO then carries `incomplete` with a stable reason, a user-facing report-a-bug message
//!   and source-delta byte/line ranges (positions only; never source text).
//!
//! `incomplete` is `null` for the other two states, so bindings can render it verbatim.
use serde_json::{json, Value};

pub const COMPLETE: &str = "complete";
pub const EQUIVALENT: &str = "equivalent";
pub const INCOMPLETE: &str = "incomplete";
/// Stable reason code: the sources differ but every draft was suppressed without proof.
pub const REASON_UNEXPLAINED_SOURCE_DELTA: &str = "unexplained_source_delta";
pub const REPORT_URL: &str = "https://github.com/buchochelliq-labs/intentumdiff-core/issues/new";
/// Bound the evidence carrier: a review that lost everything still serves bounded output.
const MAX_RANGES: usize = 64;

/// The completeness state for a review summary. `identical` is `None` when sources are
/// unavailable (an already-finalized DTO): then only the recorded `no_surviving_changes`
/// marker can establish an unexplained delta.
pub(crate) fn classify(has_changes: bool, identical: Option<bool>, is_style_only: bool,
    equivalence_proof: bool, no_surviving_changes: bool) -> &'static str {
    if has_changes || identical == Some(true) {
        COMPLETE
    } else if is_style_only || equivalence_proof {
        EQUIVALENT
    } else if identical == Some(false) || no_surviving_changes {
        INCOMPLETE
    } else {
        COMPLETE
    }
}

/// Language ids are routed identifiers; anything else is withheld from the message.
fn safe_id(value: &str) -> &str {
    let ok = !value.is_empty() && value.len() <= 64
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '.' | '#'));
    if ok { value } else { "unknown" }
}

/// The user-facing text every binding shows verbatim. It names only the language id and the
/// core version: no filename, label or source text may appear here (privacy).
pub fn incomplete_message(language: &str) -> String {
    format!(
        "IntentumDiff could not explain this change. The {language} sources differ, but no \
reviewable change survived analysis and equivalence was not proven, so this result is \
incomplete rather than a clean review. Please report a bug at {REPORT_URL} \
(reason: {REASON_UNEXPLAINED_SOURCE_DELTA}; language: {language}; core: {version}).",
        language = safe_id(language),
        version = crate::VERSION,
    )
}

fn line_of(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset.min(source.len())].iter().filter(|b| **b == b'\n').count()
}

/// Changed byte/line hunks (zero-based lines; end bytes exclusive). Reuses the invariance
/// source-evidence diff; only the spans are retained.
pub(crate) fn source_ranges(old_source: &str, new_source: &str) -> (Vec<Value>, bool) {
    let evidence = crate::invariance_groups::rust_changed_source_evidence(old_source, new_source);
    let truncated = evidence.len() > MAX_RANGES;
    let ranges = evidence.iter().take(MAX_RANGES).map(|hunk| {
        let ((os, oe), (ns, ne)) = (hunk.old_span, hunk.new_span);
        json!({"old_start_byte":os,"old_end_byte":oe,"new_start_byte":ns,"new_end_byte":ne,
            "old_start_line":line_of(old_source, os),"old_end_line":line_of(old_source, oe),
            "new_start_line":line_of(new_source, ns),"new_end_line":line_of(new_source, ne)})
    }).collect();
    (ranges, truncated)
}

fn incomplete_payload(language: &str, sources: Option<(&str, &str)>) -> Value {
    let (ranges, truncated) = sources.map_or((Vec::new(), false), |(old, new)| source_ranges(old, new));
    json!({
        "reason": REASON_UNEXPLAINED_SOURCE_DELTA,
        "message": incomplete_message(language),
        "report_url": REPORT_URL,
        "language": safe_id(language),
        "core_version": crate::VERSION,
        "source_ranges": ranges,
        "source_ranges_truncated": truncated,
        "source_ranges_available": sources.is_some(),
    })
}

/// Stamp `completeness` + `incomplete` on a served diff (idempotent). With sources the state
/// is derived from byte identity; without them an existing sourced state is kept.
pub(crate) fn attach(diff: &mut Value, sources: Option<(&str, &str)>) {
    let Some(object) = diff.as_object() else { return };
    let has_changes = object.get("changes").and_then(Value::as_array).is_some_and(|c| !c.is_empty());
    let style = object.get("is_style_only").and_then(Value::as_bool).unwrap_or(false);
    let metadata = object.get("metadata");
    let proof = metadata.and_then(|m| m.get("equivalence_proof")).is_some_and(|p| p.is_string());
    let nsc = metadata.and_then(|m| m.get("no_surviving_changes")).and_then(Value::as_bool).unwrap_or(false);
    let previous = object.get("completeness").and_then(Value::as_str);
    let state = match sources {
        Some((old, new)) => classify(has_changes, Some(old == new), style, proof, nsc),
        // A sourced classification survives later source-less stages unless the change
        // list itself now explains the review.
        None if has_changes => COMPLETE,
        None => match previous {
            Some(COMPLETE) => COMPLETE,
            Some(EQUIVALENT) => EQUIVALENT,
            Some(INCOMPLETE) => INCOMPLETE,
            _ => classify(false, None, style, proof, nsc),
        },
    };
    let language = object.get("language").and_then(Value::as_str).unwrap_or("").to_owned();
    let keep_evidence = sources.is_none() && previous == Some(INCOMPLETE)
        && object.get("incomplete").is_some_and(Value::is_object);
    let incomplete = if state != INCOMPLETE {
        Value::Null
    } else if keep_evidence {
        object["incomplete"].clone()
    } else {
        incomplete_payload(&language, sources)
    };
    diff["completeness"] = json!(state);
    diff["incomplete"] = incomplete;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(changes: Value, style: bool, metadata: Value) -> Value {
        json!({"language":"python","changes":changes,"is_style_only":style,"metadata":metadata})
    }

    #[test]
    fn states_are_distinguishable() {
        let mut same = diff(json!([]), true, json!({}));
        attach(&mut same, Some(("x = 1\n", "x = 1\n")));
        assert_eq!(same["completeness"], COMPLETE);
        assert!(same["incomplete"].is_null());
        let mut style = diff(json!([]), true, json!({}));
        attach(&mut style, Some(("x = 1\n", "x  =  1\n")));
        assert_eq!(style["completeness"], EQUIVALENT);
        let mut proof = diff(json!([]), false, json!({"equivalence_proof":"rule"}));
        attach(&mut proof, Some(("a\n", "b\n")));
        assert_eq!(proof["completeness"], EQUIVALENT);
        let mut changed = diff(json!([{"change_type":"MODIFICATION"}]), false, json!({}));
        attach(&mut changed, Some(("a\n", "b\n")));
        assert_eq!(changed["completeness"], COMPLETE);
        let mut lost = diff(json!([]), false, json!({}));
        attach(&mut lost, Some(("x = 1\nsecret = 2\n", "x = 1\nsecret = 3\n")));
        assert_eq!(lost["completeness"], INCOMPLETE);
        let incomplete = &lost["incomplete"];
        assert_eq!(incomplete["reason"], REASON_UNEXPLAINED_SOURCE_DELTA);
        assert_eq!(incomplete["report_url"], REPORT_URL);
        let message = incomplete["message"].as_str().unwrap();
        assert!(message.contains(REPORT_URL) && message.contains("python") && message.contains(crate::VERSION));
        assert!(!message.contains("secret"), "{message}");
        assert_eq!(incomplete["source_ranges"], json!([{"old_start_byte":6,"old_end_byte":17,
            "new_start_byte":6,"new_end_byte":17,"old_start_line":1,"old_end_line":2,
            "new_start_line":1,"new_end_line":2}]));
        // Source-less stages keep the sourced verdict and its evidence.
        let before = lost.clone();
        attach(&mut lost, None);
        assert_eq!(lost, before);
        let mut style_again = style.clone();
        attach(&mut style_again, None);
        assert_eq!(style_again, style);
        let mut same_again = same.clone();
        attach(&mut same_again, None);
        assert_eq!(same_again["completeness"], COMPLETE);
    }

    #[test]
    fn sourceless_marker_is_incomplete_and_language_is_sanitized() {
        let mut lost = json!({"language":"evil\nsecret","changes":[],"is_style_only":false,
            "metadata":{"no_surviving_changes":true}});
        attach(&mut lost, None);
        assert_eq!(lost["completeness"], INCOMPLETE);
        assert_eq!(lost["incomplete"]["language"], "unknown");
        assert_eq!(lost["incomplete"]["source_ranges_available"], false);
        assert!(!lost["incomplete"]["message"].as_str().unwrap().contains("secret"));
    }
}
