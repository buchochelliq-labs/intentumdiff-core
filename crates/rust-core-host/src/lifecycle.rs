//! Shared file lifecycle decisions used by every public API.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileLifecycle {
    Added,
    Deleted,
    Modified,
}

impl FileLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Modified => "modified",
        }
    }
}

/// Infer from source presence and explicit version-control status, including empty added files.
pub fn infer(old_source: &str, new_source: &str, status: Option<&str>) -> FileLifecycle {
    match crate::infer_file_lifecycle(None, old_source, new_source, status) {
        "added" => FileLifecycle::Added,
        "deleted" => FileLifecycle::Deleted,
        _ => FileLifecycle::Modified,
    }
}

/// Apply lifecycle classification while retaining source changes and unrelated metadata.
pub fn finalize(mut diff: Value, lifecycle: FileLifecycle) -> Result<Value, String> {
    if !diff.is_object() {
        return Err("diff must be an object".into());
    }
    for field in ["changes", "change_groups"] {
        if !diff.get(field).is_some_and(Value::is_array) {
            return Err(format!("diff.{field} must be an array"));
        }
    }
    crate::apply_file_lifecycle_to_diff(&mut diff, lifecycle.as_str());
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_modified_empty_files_are_not_added_or_deleted() {
        assert_eq!(infer("", "x", Some("modified")), FileLifecycle::Modified);
        assert_eq!(infer("x", "", Some("M")), FileLifecycle::Modified);
    }
}
