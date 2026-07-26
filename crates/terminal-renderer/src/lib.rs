//! Terminal renderer — emits ANSI-coloured diff output.

use serde_json::Value;

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "renderer-plugin",
});

use crate::exports::intentdiff::plugin::renderer::Guest;

const _PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

struct TerminalRenderer;

const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";

fn colour_for(change_type: &str) -> &'static str {
    match change_type {
        "ADDITION" => GREEN,
        "DELETION" => RED,
        "MODIFICATION" => YELLOW,
        "MOVE" => BLUE,
        "REFACTORING" => MAGENTA,
        _ => DIM,
    }
}

fn symbol_for(change_type: &str) -> &'static str {
    match change_type {
        "ADDITION" => "+",
        "DELETION" => "-",
        "MODIFICATION" => "~",
        "MOVE" => "→",
        "REFACTORING" => "⟳",
        _ => " ",
    }
}

pub fn render_str(diff_json: &str) -> String {
    let diff: Value = match serde_json::from_str(diff_json) {
        Ok(v) => v,
        Err(e) => return format!("[IntentDiff] ERROR: invalid diff JSON: {}", e),
    };

    let mut out = String::new();

    let old_file = diff["old_filename"].as_str().unwrap_or("old");
    let new_file = diff["new_filename"].as_str().unwrap_or("new");
    let language = diff["language"].as_str().unwrap_or("unknown");

    out.push_str(&format!(
        "{}--- {}{}\n{}+++ {}{}\n{}language: {}{}\n\n",
        BOLD, old_file, RESET, BOLD, new_file, RESET, DIM, language, RESET,
    ));

    if diff["is_style_only"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "{}(style-only diff — no semantic changes){}\n",
            DIM, RESET
        ));
        return out;
    }

    let empty = vec![];
    let changes = diff["changes"].as_array().unwrap_or(&empty);

    if changes.is_empty() {
        out.push_str(&format!("{}(no changes){}\n", DIM, RESET));
        return out;
    }

    for change in changes {
        let ct = change["change_type"].as_str().unwrap_or("?");
        let colour = colour_for(ct);
        let symbol = symbol_for(ct);
        let desc = change["description"].as_str().unwrap_or("");

        // Position info
        let pos = if let Some(new_node) = change.get("new_node").filter(|v| !v.is_null()) {
            format!(
                " [{}:{}]",
                new_node["position"]["start_line"].as_u64().unwrap_or(0) + 1,
                new_node["position"]["start_col"].as_u64().unwrap_or(0),
            )
        } else if let Some(old_node) = change.get("old_node").filter(|v| !v.is_null()) {
            format!(
                " [{}:{}]",
                old_node["position"]["start_line"].as_u64().unwrap_or(0) + 1,
                old_node["position"]["start_col"].as_u64().unwrap_or(0),
            )
        } else {
            String::new()
        };

        out.push_str(&format!(
            "{}{} [{}]{} {}{}\n",
            colour, symbol, ct, pos, desc, RESET
        ));
    }

    // Parse error warnings
    let parse_errors = diff["parse_errors"].as_array().unwrap_or(&empty);
    if !parse_errors.is_empty() {
        out.push('\n');
        for err in parse_errors {
            out.push_str(&format!(
                "{}⚠ {}{}\n",
                YELLOW,
                err.as_str().unwrap_or("?"),
                RESET
            ));
        }
    }

    out
}

impl Guest for TerminalRenderer {
    fn format_name() -> String {
        "terminal-color".to_string()
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

export!(TerminalRenderer);
