//! Human-facing CLI presentation shared by native and C-ABI consumers.
//! Formats an authoritative diff DTO; never parses source or decides equivalence.
use rich::{ColorSystem, Console, Panel, Table, Text};
use rich::console::Overflow;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Request {
    diff: Diff,
    width: usize,
    color: bool,
}
#[derive(Deserialize)]
struct Diff {
    changes: Vec<Change>,
    has_semantic_changes: bool,
    is_style_only: bool,
    is_fallback: bool,
    #[serde(default)] old_filename: String,
    #[serde(default)] new_filename: String,
    #[serde(default)] language: String,
    #[serde(default)] staging_status: Option<String>,
    #[serde(default)] parse_errors: Vec<Value>,
    #[serde(default)] guardrail_violations: Vec<Violation>,
}
#[derive(Deserialize)]
struct Change {
    change_type: String,
    #[serde(default)] description: String,
}
#[derive(Deserialize)]
struct Violation {
    severity: String,
    message: String,
    #[serde(default)] file: String,
    #[serde(default)] semantic_path: String,
    #[serde(default)] old_value: Value,
    #[serde(default)] new_value: Value,
}

// Literal Text prevents markup interpretation; make terminal controls visible too.
fn literal(value: &str) -> String {
    value.chars().flat_map(|c| {
        if c.is_control() && c != '\n' && c != '\t' { c.escape_default().collect::<Vec<_>>() }
        else { vec![c] }
    }).collect()
}
fn text(value: &str) -> Text { Text::new(literal(value)).overflow(Overflow::Fold) }
fn styled(value: &str, style: &str) -> Text {
    let mut result = Text::new("").overflow(Overflow::Fold);
    result.append(&literal(value), Some(style.to_owned().into()));
    result
}
fn append(output: &mut String, console: &Console, item: &dyn rich::Renderable) {
    output.push_str(console.render_to_string(item).trim_end_matches('\n'));
    output.push('\n');
}

/// Render `{diff: SemanticDiff, width: 16..=300, color: bool}` as terminal text.
/// The caller owns terminal detection, stdout/stderr selection and filesystem I/O.
/// Malformed DTOs are errors, never a fabricated "no changes" result.
pub fn render_cli_review_impl(request_json: &str) -> Result<String, String> {
    let request: Request = serde_json::from_str(request_json).map_err(|e| format!("CLI presentation: {e}"))?;
    if !(16..=300).contains(&request.width) { return Err("CLI width must be between 16 and 300".into()); }
    let diff = request.diff;
    let console = Console::builder().width(request.width).force_terminal(request.color)
        .color_system(if request.color { Some(ColorSystem::Standard) } else { None })
        .no_color(!request.color).markup(false).build();
    let mut output = String::new();
    if !diff.guardrail_violations.is_empty() {
        let mut table = Table::new().title("Protected semantic changes");
        table.add_column("Severity").add_column("Location").add_column("Message / value");
        for item in &diff.guardrail_violations {
            let values = if item.old_value.is_null() && item.new_value.is_null() { String::new() }
                else { format!("\n{} -> {}", item.old_value, item.new_value) };
            if request.width < 60 {
                let details = format!("{}\n{}::{}\n{}{values}", item.severity.to_uppercase(), item.file, item.semantic_path, item.message);
                append(&mut output, &console, &Panel::new(Box::new(text(&details))).title("Guardrail").border_style("red"));
            } else {
                table.add_row_text(vec![styled(&item.severity.to_uppercase(), "bold red"),
                    text(&format!("{}::{}", item.file, item.semantic_path)), text(&format!("{}{values}", item.message))]);
            }
        }
        if request.width >= 60 { append(&mut output, &console, &table); }
    }
    let (title, state, style) = if diff.is_fallback {
        ("Source fallback", "Semantic equivalence unknown; source changes require review.", "yellow")
    } else if diff.is_style_only {
        ("Style-only change", "Formatting changed; no semantic differences were found.", "yellow")
    } else if !diff.has_semantic_changes && diff.changes.is_empty() {
        ("No changes detected", "No semantic differences were found.", "green")
    } else { ("Semantic diff", "", "cyan") };
    let mut summary = text(state);
    if !state.is_empty() { summary.append("\n", None); }
    for (label, value) in [("Old", &diff.old_filename), ("New", &diff.new_filename), ("Language", &diff.language)] {
        summary.append(&format!("{label}: {}\n", literal(value)), None);
    }
    if let Some(scope) = &diff.staging_status {
        summary.append(&format!("Scope: {}\n", literal(&scope.replace('_', " "))), None);
    }
    summary.append(&format!("Changes: {}", diff.changes.len()), None);
    let panel = Panel::new(Box::new(summary)).title_as_text(styled(title, &format!("bold {style}"))).border_style(style);
    append(&mut output, &console, &panel);
    if !diff.parse_errors.is_empty() {
        append(&mut output, &console, &styled(&format!("Parse warnings: {}", diff.parse_errors.len()), "yellow"));
    }
    if !diff.changes.is_empty() {
        let mut table = Table::new();
        table.add_column("Type").add_column("Description");
        for change in &diff.changes {
            let style = match change.change_type.as_str() {
                "ADDITION" => "green", "DELETION" => "red", "MODIFICATION" => "yellow",
                "MOVE" => "cyan", "REFACTORING" => "magenta", _ => "white",
            };
            if request.width < 40 {
                let details = format!("{}\n{}", change.change_type, change.description);
                append(&mut output, &console, &Panel::new(Box::new(text(&details))).title("Change").border_style(style));
            } else {
                table.add_row_text(vec![styled(&change.change_type, style), text(&change.description)]);
            }
        }
        if request.width >= 40 { append(&mut output, &console, &table); }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn request() -> Value { json!({"width":80,"color":false,"diff":{
        "changes":[{"change_type":"MODIFICATION","description":"Update [red]name[/red] to 中文"}],
        "has_semantic_changes":true,"is_style_only":false,"is_fallback":false,
        "old_filename":"before.py","new_filename":"after.py","language":"python"}}) }
    #[test]
    fn literal_plain_and_colored_content() {
        let mut input = request();
        let plain = render_cli_review_impl(&input.to_string()).unwrap();
        assert!(plain.contains("Semantic diff") && plain.contains("MODIFICATION"));
        assert!(plain.contains("[red]name[/red]") && plain.contains("中文"));
        assert!(!plain.contains('\x1b'));
        input["color"] = json!(true);
        assert!(render_cli_review_impl(&input.to_string()).unwrap().contains('\x1b'));
    }
    #[test]
    fn fallback_never_claims_semantic_equivalence() {
        let mut input = request(); input["diff"]["is_fallback"] = json!(true);
        let output = render_cli_review_impl(&input.to_string()).unwrap();
        assert!(output.contains("Source fallback") && output.contains("Semantic equivalence unknown"));
        assert!(!output.contains("Semantic diff"));
    }
    #[test]
    fn narrow_guardrails_keep_real_zero_values_and_wrap() {
        let mut input = request(); input["width"] = json!(40);
        input["diff"]["changes"][0]["description"] = json!("A deliberately long description that must wrap within the terminal");
        input["diff"]["guardrail_violations"] = json!([{
            "severity":"immutable", "message":"Protected value changed", "file":"policy.json",
            "semantic_path":"threshold", "old_value":0, "new_value":false
        }]);
        let output = render_cli_review_impl(&input.to_string()).unwrap();
        assert!(output.contains("IMMUTABLE") && output.contains("false"));
        assert!(output.contains("0 ->"));
        assert!(output.lines().all(|line| line.chars().count() <= 40), "{output}");
    }

    #[test]
    fn minimum_width_preserves_evidence_without_ellipsis() {
        let mut input = request(); input["width"] = json!(16);
        input["diff"]["changes"][0]["description"] = json!("Update important_protected_setting to false");
        input["diff"]["guardrail_violations"] = json!([{
            "severity":"immutable", "message":"Protected value changed", "file":"policy.json",
            "semantic_path":"important_setting", "old_value":0, "new_value":false
        }]);
        let output = render_cli_review_impl(&input.to_string()).unwrap();
        assert!(!output.contains('…'), "{output}");
        let compact: String = output.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        for expected in ["IMMUTABLE", "policyjsonimportantsetting", "Protectedvaluechanged", "0false", "Updateimportantprotectedsettingtofalse"] {
            assert!(compact.contains(expected), "missing {expected}: {output}");
        }
        assert!(output.lines().all(|line| line.chars().count() <= 16), "{output}");
    }

    #[test]
    fn malformed_dto_fails_and_controls_are_visible() {
        assert!(render_cli_review_impl(r#"{"diff":{},"width":80,"color":false}"#).is_err());
        let mut input = request(); input["diff"]["new_filename"] = json!("\u{1b}]0;injected\u{7}");
        let output = render_cli_review_impl(&input.to_string()).unwrap();
        assert!(!output.contains('\x1b') && !output.contains('\x07'));
        input["width"] = json!(0);
        assert!(render_cli_review_impl(&input.to_string()).is_err());
    }
}
