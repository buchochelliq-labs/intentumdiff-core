//! LLM renderer — emits a structured natural-language Markdown summary.

use serde_json::Value;

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "renderer-plugin",
});

use crate::exports::intentumdiff::plugin::renderer::Guest;

const _PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

struct LlmRenderer;

pub fn render_str(diff_json: &str) -> String {
    let diff: Value = match serde_json::from_str(diff_json) {
        Ok(v) => v,
        Err(e) => return format!("<!-- ERROR: {} -->", e),
    };

    let old_file = diff["old_filename"].as_str().unwrap_or("old");
    let new_file = diff["new_filename"].as_str().unwrap_or("new");
    let language = diff["language"].as_str().unwrap_or("unknown");

    let mut md = String::new();
    md.push_str("## Semantic Diff Summary\n\n");
    md.push_str(&format!(
        "**File:** `{}` → `{}`  \n**Language:** {}  \n",
        old_file, new_file, language
    ));

    if diff["is_style_only"].as_bool().unwrap_or(false) {
        md.push_str(
            "\n> **Style-only diff** — the code is semantically identical. \
             Only formatting, whitespace, or comments changed.\n",
        );
        return md;
    }

    let empty = vec![];
    let changes = diff["changes"].as_array().unwrap_or(&empty);

    if changes.is_empty() {
        md.push_str("\n> No semantic changes detected.\n");
        return md;
    }

    md.push_str(&format!(
        "**Total semantic changes:** {}\n\n",
        changes.len()
    ));

    let categories = [
        "ADDITION",
        "DELETION",
        "MODIFICATION",
        "MOVE",
        "REFACTORING",
    ];

    for cat in &categories {
        let cat_changes: Vec<&Value> = changes
            .iter()
            .filter(|c| c["change_type"].as_str() == Some(cat))
            .collect();

        if cat_changes.is_empty() {
            continue;
        }

        let heading = match *cat {
            "ADDITION" => "Additions",
            "DELETION" => "Deletions",
            "MODIFICATION" => "Modifications",
            "MOVE" => "Moves",
            "REFACTORING" => "Refactorings",
            _ => cat,
        };

        md.push_str(&format!("### {} ({})\n\n", heading, cat_changes.len()));

        for change in cat_changes {
            let desc = change["description"].as_str().unwrap_or("");
            let conf = change["confidence"].as_f64().unwrap_or(1.0);

            let loc = if let Some(n) = change.get("new_node").filter(|v| !v.is_null()) {
                let line = n["position"]["start_line"].as_u64().unwrap_or(0) + 1;
                format!(" at line {}", line)
            } else if let Some(n) = change.get("old_node").filter(|v| !v.is_null()) {
                let line = n["position"]["start_line"].as_u64().unwrap_or(0) + 1;
                format!(" at line {}", line)
            } else {
                String::new()
            };

            let conf_note = if conf < 0.9 {
                format!(" _(confidence: {:.0}%)_", conf * 100.0)
            } else {
                String::new()
            };

            md.push_str(&format!("- {}{}{}\n", desc, loc, conf_note));
        }

        md.push('\n');
    }

    // Parse error warnings
    let parse_errors = diff["parse_errors"].as_array().unwrap_or(&empty);
    if !parse_errors.is_empty() {
        md.push_str("### ⚠ Parse Errors\n\n");
        for err in parse_errors {
            md.push_str(&format!("- {}\n", err.as_str().unwrap_or("?")));
        }
        md.push('\n');
    }

    md
}

impl Guest for LlmRenderer {
    fn format_name() -> String {
        "llm".to_string()
    }
    fn render(diff_json: String) -> String {
        render_str(&diff_json)
    }
    fn supported_options() -> Vec<String> {
        vec![]
    }
    fn priority() -> i32 {
        0
    }
}

export!(LlmRenderer);
