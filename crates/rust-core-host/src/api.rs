//! Supported in-process Rust review API. No Python or ABI serialization is required.
//!
//! The C ABI and this facade delegate to the same engine. Extensible metadata is
//! JSON by design; review results, source positions, nodes and errors are typed.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Debug)]
#[non_exhaustive]
pub enum ReviewError {
    Engine(String),
    Unsupported(String),
    InvalidResult(serde_json::Error),
}
impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(s) | Self::Unsupported(s) => f.write_str(s),
            Self::InvalidResult(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for ReviewError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub start_line: u32,
    /// UTF-8 byte column, not a character index.
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub node_type: String,
    pub label: String,
    pub position: Position,
    pub structural_hash: String,
    #[serde(default)]
    pub children: Vec<Node>,
    #[serde(flatten)]
    pub attributes: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    /// Open vocabulary: new engine classifications remain forward-compatible.
    pub change_type: String,
    pub old_node: Option<Node>,
    pub new_node: Option<Node>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(flatten)]
    pub attributes: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeGroup {
    pub kind: String,
    #[serde(default)]
    pub raw_change_indices: Vec<usize>,
    #[serde(flatten)]
    pub attributes: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub old_filename: String,
    pub new_filename: String,
    pub language: String,
    pub changes: Vec<Change>,
    pub change_groups: Vec<ChangeGroup>,
    pub has_semantic_changes: bool,
    pub is_style_only: bool,
    pub is_fallback: bool,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(flatten)]
    pub attributes: Map<String, Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detect_refactorings: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_to_token_diff: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_nodes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guardrails_enabled: Option<bool>,
    /// Additional engine configuration, using the same documented names as the ABI.
    /// Empty options use engine defaults; unknown result fields are preserved.
    #[serde(flatten)]
    pub settings: Map<String, Value>,
}

/// Plain text and Markdown review. Language-aware review uses `review_sources`.
pub fn review_text(
    old: &str,
    new: &str,
    old_filename: &str,
    new_filename: &str,
) -> Result<Review, ReviewError> {
    serde_json::from_value(
        crate::review_text(old, new, old_filename, new_filename).map_err(ReviewError::Engine)?,
    )
    .map_err(ReviewError::InvalidResult)
}

/// Review two source versions with local parser components and repository context.
/// Schema/compile discovery and guardrails follow the native live-review contract.
pub fn review_sources(
    repo_root: &Path,
    parser_dir: &Path,
    filename: &str,
    old: &str,
    new: &str,
    options: &ReviewOptions,
) -> Result<Review, ReviewError> {
    let config = serde_json::to_string(options).map_err(ReviewError::InvalidResult)?;
    let raw = crate::live_server::live_diff_contents_impl(
        &repo_root.to_string_lossy(),
        filename,
        old,
        new,
        &config,
        &parser_dir.to_string_lossy(),
    )
    .map_err(ReviewError::Engine)?;
    let mut envelope: Value = serde_json::from_str(&raw).map_err(ReviewError::InvalidResult)?;
    if let Some(reason) = envelope.get("fallback").and_then(Value::as_str) {
        return Err(ReviewError::Unsupported(reason.to_owned()));
    }
    serde_json::from_value(envelope["diff"].take()).map_err(ReviewError::InvalidResult)
}
