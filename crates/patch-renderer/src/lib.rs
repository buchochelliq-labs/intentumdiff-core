//! Patch renderer — emits a structured semantic patch.

use serde_json::Value;

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "renderer-plugin",
});

use crate::exports::intentdiff::plugin::renderer::Guest;

const _PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

struct PatchRenderer;

pub fn render_str(diff_json: &str) -> String {
    let diff: Value = match serde_json::from_str(diff_json) {
        Ok(v) => v,
        Err(e) => return format!("# ERROR: invalid diff JSON: {}\n", e),
    };

    let old_file = diff["old_filename"].as_str().unwrap_or("old");
    let new_file = diff["new_filename"].as_str().unwrap_or("new");
    let language = diff["language"].as_str().unwrap_or("unknown");

    let mut out = String::new();
    out.push_str(&format!(
        "--- {}\n+++ {}\n# language: {}\n\n",
        old_file, new_file, language
    ));

    if diff["is_style_only"].as_bool().unwrap_or(false) {
        out.push_str("# style-only diff — no semantic changes\n");
        return out;
    }

    let empty = vec![];
    let changes = diff["changes"].as_array().unwrap_or(&empty);

    for change in changes {
        let ct = change["change_type"].as_str().unwrap_or("?");
        let desc = change["description"].as_str().unwrap_or("");
        let confidence = change["confidence"].as_f64().unwrap_or(1.0);

        out.push_str(&format!(
            "@@ {} [confidence={:.2}]\n# {}\n",
            ct, confidence, desc
        ));

        if let Some(old_node) = change.get("old_node").filter(|v| !v.is_null()) {
            let label = old_node["label"].as_str().unwrap_or("");
            let sl = old_node["position"]["start_line"].as_u64().unwrap_or(0) + 1;
            out.push_str(&format!("- {} (line {})\n", label, sl));
        }

        if let Some(new_node) = change.get("new_node").filter(|v| !v.is_null()) {
            let label = new_node["label"].as_str().unwrap_or("");
            let sl = new_node["position"]["start_line"].as_u64().unwrap_or(0) + 1;
            out.push_str(&format!("+ {} (line {})\n", label, sl));
        }

        out.push('\n');
    }

    out
}

impl Guest for PatchRenderer {
    fn format_name() -> String {
        "patch".to_string()
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

export!(PatchRenderer);
