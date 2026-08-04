//! HTML renderer — emits a self-contained HTML table of semantic changes.

use serde_json::Value;

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "renderer-plugin",
});

use crate::exports::intentdiff::plugin::renderer::Guest;

const _PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

struct HtmlRenderer;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn css_class(change_type: &str) -> &'static str {
    match change_type {
        "ADDITION" => "addition",
        "DELETION" => "deletion",
        "MODIFICATION" => "modification",
        "MOVE" => "move",
        "REFACTORING" => "refactoring",
        _ => "style-only",
    }
}

pub fn render_str(diff_json: &str) -> String {
    let diff: Value = match serde_json::from_str(diff_json) {
        Ok(v) => v,
        Err(e) => {
            return format!(
                "<p class=\"error\">Invalid diff JSON: {}</p>",
                escape(&e.to_string())
            )
        }
    };

    let old_file = escape(diff["old_filename"].as_str().unwrap_or("old"));
    let new_file = escape(diff["new_filename"].as_str().unwrap_or("new"));
    let language = escape(diff["language"].as_str().unwrap_or("unknown"));

    let mut html = String::new();
    html.push_str(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Semantic Diff</title>
<style>
body{font-family:monospace;margin:2em}
table{border-collapse:collapse;width:100%}
th,td{border:1px solid #ccc;padding:.4em .8em;text-align:left}
tr.addition td{background:#e6ffed}
tr.deletion td{background:#ffeef0}
tr.modification td{background:#fff5b1}
tr.move td{background:#dbedff}
tr.refactoring td{background:#f0dbff}
tr.style-only td{color:#888}
.badge{border-radius:3px;padding:1px 6px;font-size:.85em;color:#fff;font-weight:bold}
.addition .badge{background:#28a745}
.deletion .badge{background:#d73a49}
.modification .badge{background:#dbab09;color:#000}
.move .badge{background:#0366d6}
.refactoring .badge{background:#6f42c1}
</style>
</head>
<body>
"#,
    );

    html.push_str(&format!(
        "<h2>Semantic Diff</h2>\n\
         <p><strong>Old:</strong> {old_file}<br><strong>New:</strong> {new_file}<br>\
         <strong>Language:</strong> {language}</p>\n"
    ));

    if diff["is_style_only"].as_bool().unwrap_or(false) {
        html.push_str("<p><em>Style-only diff — no semantic changes.</em></p>");
        html.push_str("</body></html>");
        return html;
    }

    let empty = vec![];
    let changes = diff["changes"].as_array().unwrap_or(&empty);

    html.push_str(
        "<table><thead><tr><th>Type</th><th>Description</th><th>Old location</th>\
         <th>New location</th><th>Confidence</th></tr></thead><tbody>\n",
    );

    for change in changes {
        let ct = change["change_type"].as_str().unwrap_or("?");
        let cls = css_class(ct);
        let desc = escape(change["description"].as_str().unwrap_or(""));
        let conf = change["confidence"].as_f64().unwrap_or(1.0);

        let old_loc = if let Some(n) = change.get("old_node").filter(|v| !v.is_null()) {
            format!(
                "{}:{}",
                n["position"]["start_line"].as_u64().unwrap_or(0) + 1,
                n["position"]["start_col"].as_u64().unwrap_or(0),
            )
        } else {
            String::new()
        };

        let new_loc = if let Some(n) = change.get("new_node").filter(|v| !v.is_null()) {
            format!(
                "{}:{}",
                n["position"]["start_line"].as_u64().unwrap_or(0) + 1,
                n["position"]["start_col"].as_u64().unwrap_or(0),
            )
        } else {
            String::new()
        };

        let ct_escaped = escape(ct);
        html.push_str(&format!(
            "<tr class=\"{cls}\"><td><span class=\"badge\">{ct_escaped}</span></td>\
             <td>{desc}</td><td>{old_loc}</td><td>{new_loc}</td>\
             <td>{conf:.0}%</td></tr>\n",
            conf = conf * 100.0,
        ));
    }

    html.push_str("</tbody></table>\n</body></html>\n");
    html
}

impl Guest for HtmlRenderer {
    fn format_name() -> String {
        "html".to_string()
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

export!(HtmlRenderer);
