//! Sanitised repro outline for an incomplete review (core #252).
//!
//! Core builds the outline once so every binding scrubs the same way. Bindings preview the
//! rendered text, ask for explicit confirmation, then open the pre-filled issue URL; core never
//! sends anything itself.
//!
//! Privacy contract: no user source text anywhere. The outline carries only the core version,
//! the routed language id, the parser plugin id/version, grammar node kinds with nesting
//! around each changed region, the passes that removed draft changes (pass names, change
//! types, node kinds), and line/column offsets and byte sizes. Identifiers become per-report
//! placeholders (`id1`, `id2`, …; never hashes), literals become their type and length
//! (`string(12)`, `int`, `float`), and comments/docstrings are dropped. A node kind that is not
//! plain grammar vocabulary, or that equals an identifier's text, is shown as `node`.
//!
//! Deterministic: the same inputs give a byte-identical outline, body and URL.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

/// Maximum length of the pre-filled issue URL, in bytes.
pub const MAX_ISSUE_URL_BYTES: usize = 8000;
const MAX_DEPTH: usize = 5;
const MAX_CHILDREN: usize = 8;
const MAX_SKELETON: usize = 12;
const TRUNCATION_NOTE: &str =
    "_(Outline truncated to fit the issue URL. Your IntentumDiff client can copy the full outline.)_";

/// Trees and suppression record a routed review hands the outline builder.
pub(crate) struct ReproContext<'a> {
    pub old_tree: &'a Value,
    pub new_tree: &'a Value,
    /// `[{pass, removed:[{change_type,node_type,count}], added:[…]}]`, or `null` when the
    /// route recorded no trace.
    pub suppression_trace: &'a Value,
}

#[derive(Default)]
struct Placeholders {
    ids: BTreeMap<String, usize>,
}

impl Placeholders {
    fn get(&mut self, text: &str) -> String {
        let next = self.ids.len() + 1;
        format!("id{}", self.ids.entry(text.to_owned()).or_insert(next))
    }
}

fn line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0).chain(source.match_indices('\n').map(|(i, _)| i + 1)).collect()
}

fn span(node: &Value, starts: &[usize], len: usize) -> Option<(usize, usize)> {
    let p = node.get("position")?;
    let at = |line: &str, col: &str| -> Option<usize> {
        let line = usize::try_from(p.get(line)?.as_u64()?).ok()?;
        let col = usize::try_from(p.get(col)?.as_u64()?).ok()?;
        Some((starts.get(line)? + col).min(len))
    };
    Some((at("start_line", "start_col")?, at("end_line", "end_col")?))
}

fn node_type(node: &Value) -> &str {
    node.get("node_type").and_then(Value::as_str).unwrap_or("")
}

fn label(node: &Value) -> &str {
    node.get("label").and_then(Value::as_str).unwrap_or("")
}

fn children(node: &Value) -> &[Value] {
    node.get("children").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn is_comment(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("comment") || kind.contains("docstring")
}

/// Literal kind for a node type, if it denotes a literal value.
fn literal_kind(kind: &str) -> Option<&'static str> {
    let kind = kind.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| kind.contains(w));
    if has(&["integer", "int_literal", "int_lit"]) || kind == "int" {
        Some("int")
    } else if has(&["float", "decimal", "real_literal", "double"]) {
        Some("float")
    } else if has(&["number", "numeric", "digit"]) {
        Some("number")
    } else if has(&["bool", "true", "false"]) {
        Some("bool")
    } else if has(&["null", "nil"]) || kind == "none" {
        Some("null")
    } else if has(&["string", "char", "literal", "scalar", "heredoc", "template"])
        || kind == "text" || kind.ends_with("_text") || kind.starts_with("text_") {
        Some("string")
    } else {
        None
    }
}

fn identifier_like(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$' || c == '@')
        && text.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '@' | '-' | '.' | ':'))
}

/// A leaf (or entity) label that names something the user wrote.
fn user_name(node: &Value) -> Option<&str> {
    let (kind, text) = (node_type(node), label(node));
    (!text.is_empty() && !text.eq_ignore_ascii_case(kind) && literal_kind(kind).is_none()
        && !is_comment(kind) && identifier_like(text)
        && !crate::keyed_data::keyed_generic_label(text, kind)).then_some(text)
}

/// Every user-written name in a tree: node kinds equal to one of these are withheld.
fn collect_names<'a>(node: &'a Value, names: &mut HashSet<&'a str>) {
    if let Some(name) = user_name(node) {
        names.insert(name);
    }
    for child in children(node) {
        collect_names(child, names);
    }
}

struct Side<'a> {
    source: &'a str,
    starts: Vec<usize>,
}

struct Builder<'a> {
    unsafe_kinds: HashSet<&'a str>,
    placeholders: Placeholders,
}

impl<'a> Builder<'a> {
    fn kind(&self, raw: &str) -> String {
        let grammar = !raw.is_empty() && raw.len() <= 48
            && raw.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            && raw.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if grammar && !self.unsafe_kinds.contains(raw) { raw.to_owned() } else { "node".to_owned() }
    }

    /// A bare string as a whole statement is a docstring or comment-like: dropped.
    fn dropped(node: &Value) -> bool {
        let kind = node_type(node);
        is_comment(kind) || (kind.contains("expression_statement") && children(node).len() == 1
            && literal_kind(node_type(&children(node)[0])) == Some("string"))
    }

    fn render(&mut self, node: &Value, side: &Side<'_>, depth: usize) -> String {
        let raw = node_type(node);
        if let Some(literal) = literal_kind(raw) {
            // Descend through single-literal wrappers to the most specific kind.
            let inner = children(node);
            if inner.len() == 1 && literal_kind(node_type(&inner[0])).is_some() {
                return self.render(&inner[0], side, depth);
            }
            return if literal == "string" {
                let bytes = span(node, &side.starts, side.source.len()).map_or(0, |(s, e)| e.saturating_sub(s));
                format!("string({bytes})")
            } else {
                literal.to_owned()
            };
        }
        let kids: Vec<&Value> = children(node).iter().filter(|c| !Self::dropped(c)).collect();
        if kids.is_empty() {
            return match user_name(node) {
                Some(name) => self.placeholders.get(name),
                None => self.kind(raw),
            };
        }
        let mut head = self.kind(raw);
        if let Some(name) = user_name(node) {
            head = format!("{head}[{}]", self.placeholders.get(name));
        }
        if depth >= MAX_DEPTH {
            return format!("{head}(…)");
        }
        let mut parts: Vec<String> = kids.iter().take(MAX_CHILDREN).map(|c| self.render(c, side, depth + 1)).collect();
        if kids.len() > MAX_CHILDREN {
            parts.push(format!("…+{}", kids.len() - MAX_CHILDREN));
        }
        format!("{head}({})", parts.join(", "))
    }

    /// Ancestor kinds down to the deepest node covering `[start, end)`, and that node's shape.
    fn locate(&mut self, tree: &Value, side: &Side<'_>, start: usize, end: usize) -> (String, String) {
        let mut path = vec![tree];
        loop {
            let current = *path.last().unwrap();
            let next = children(current).iter().find(|child| {
                !Self::dropped(child) && span(child, &side.starts, side.source.len())
                    .is_some_and(|(s, e)| s <= start && end <= e && (s < e || start == end))
            });
            match next {
                Some(child) => path.push(child),
                None => break,
            }
        }
        let mut kinds: Vec<String> = path.iter().map(|n| self.kind(node_type(n))).collect();
        if kinds.len() > MAX_SKELETON {
            kinds.drain(..kinds.len() - MAX_SKELETON);
            kinds.insert(0, "…".to_owned());
        }
        let shape = self.render(path.last().unwrap(), side, 0);
        (kinds.join(" > "), shape)
    }
}

fn point(starts: &[usize], offset: usize) -> (usize, usize) {
    let line = starts.partition_point(|s| *s <= offset).saturating_sub(1);
    (line, offset - starts[line])
}

fn region_side(starts: &[usize], start: usize, end: usize) -> Value {
    let (start_line, start_col) = point(starts, start);
    let (end_line, end_col) = point(starts, end);
    json!({"start_line":start_line,"start_col":start_col,"end_line":end_line,"end_col":end_col,"bytes":end - start})
}

fn sanitize_trace(trace: &Value, builder: &Builder<'_>) -> Value {
    let Some(passes) = trace.as_array() else { return Value::Null };
    let pass_name = |name: &str| -> String {
        if name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | ':' | '.'))
            && !name.is_empty() && name.len() <= 80 { name.to_owned() } else { "pass".to_owned() }
    };
    let kinds = |items: &Value| -> Value {
        json!(items.as_array().into_iter().flatten().map(|item| json!({
            "change_type": builder.kind(&item["change_type"].as_str().unwrap_or("").to_ascii_lowercase()).to_ascii_uppercase(),
            "node_type": builder.kind(item["node_type"].as_str().unwrap_or("")),
            "count": item["count"].as_u64().unwrap_or(0),
        })).collect::<Vec<_>>())
    };
    json!(passes.iter().map(|pass| json!({
        "pass": pass_name(pass["pass"].as_str().unwrap_or("")),
        "removed": kinds(&pass["removed"]),
        "added": kinds(&pass["added"]),
    })).collect::<Vec<_>>())
}

/// Build the structured outline. `ranges` are the changed byte spans `((old), (new))`.
pub(crate) fn outline(language: &str, old_source: &str, new_source: &str,
    ranges: &[((usize, usize), (usize, usize))], truncated: bool, context: Option<&ReproContext<'_>>) -> Value {
    let mut names = HashSet::new();
    if let Some(ctx) = context {
        collect_names(ctx.old_tree, &mut names);
        collect_names(ctx.new_tree, &mut names);
    }
    let mut builder = Builder { unsafe_kinds: names, placeholders: Placeholders::default() };
    let old = Side { source: old_source, starts: line_starts(old_source) };
    let new = Side { source: new_source, starts: line_starts(new_source) };
    let regions: Vec<Value> = ranges.iter().enumerate().map(|(index, ((os, oe), (ns, ne)))| {
        let mut region = json!({"index": index + 1,
            "old": region_side(&old.starts, *os, *oe),
            "new": region_side(&new.starts, *ns, *ne)});
        if let Some(ctx) = context {
            let (skeleton, shape) = builder.locate(ctx.old_tree, &old, *os, *oe);
            region["old_skeleton"] = json!(skeleton);
            region["old_shape"] = json!(shape);
            let (skeleton, shape) = builder.locate(ctx.new_tree, &new, *ns, *ne);
            region["new_skeleton"] = json!(skeleton);
            region["new_shape"] = json!(shape);
        }
        region
    }).collect();
    let suppressions = context.map_or(Value::Null, |ctx| sanitize_trace(ctx.suppression_trace, &builder));
    json!({
        "schema_version": 1,
        "reason": crate::completeness::REASON_UNEXPLAINED_SOURCE_DELTA,
        "core_version": crate::VERSION,
        "language": crate::completeness::safe_id(language),
        "plugin": {"id": Value::Null, "version": Value::Null},
        "regions": regions,
        "regions_truncated": truncated,
        "structure_available": context.is_some(),
        "suppressions": suppressions,
        "placeholder_count": builder.placeholders.ids.len(),
    })
}

fn code(value: &str) -> String {
    format!("`{}`", value.replace('`', "'"))
}

fn position(side: &Value) -> String {
    format!("{}:{}–{}:{} ({} bytes)", side["start_line"], side["start_col"], side["end_line"],
        side["end_col"], side["bytes"])
}

/// Title and Markdown body lines for an outline.
fn render_lines(outline: &Value) -> (String, Vec<String>) {
    let text = |value: &Value| value.as_str().unwrap_or("unknown").to_owned();
    let language = text(&outline["language"]);
    let title = format!("Incomplete review: {} ({language}, core {})", text(&outline["reason"]), text(&outline["core_version"]));
    let plugin = match (outline["plugin"]["id"].as_str(), outline["plugin"]["version"].as_str()) {
        (Some(id), Some(version)) => format!("{} (version {})", code(id), code(version)),
        (Some(id), None) => format!("{} (version unknown)", code(id)),
        _ => "unknown".to_owned(),
    };
    let mut lines = vec![
        "### IntentumDiff could not explain a change".to_owned(),
        String::new(),
        "The sources differ, but no reviewable change survived analysis and equivalence was not proven.".to_owned(),
        String::new(),
        format!("- Reason: {}", code(&text(&outline["reason"]))),
        format!("- Core: {}", code(&text(&outline["core_version"]))),
        format!("- Language: {}", code(&language)),
        format!("- Parser plugin: {plugin}"),
        String::new(),
        "#### Changed regions".to_owned(),
        "_Positions are zero-based `line:byte-column`; shapes show grammar node kinds only._".to_owned(),
    ];
    for region in outline["regions"].as_array().into_iter().flatten() {
        lines.push(format!("{}. old {} → new {}", region["index"], position(&region["old"]), position(&region["new"])));
        for key in ["old_skeleton", "new_skeleton", "old_shape", "new_shape"] {
            if let Some(value) = region[key].as_str() {
                lines.push(format!("   - {}: {}", key.replace('_', " "), code(value)));
            }
        }
    }
    if outline["regions_truncated"] == true {
        lines.push("- _(further regions omitted)_".to_owned());
    }
    lines.push(String::new());
    lines.push("#### Suppressed draft changes".to_owned());
    match outline["suppressions"].as_array() {
        None => lines.push("_(not recorded on this route)_".to_owned()),
        Some(passes) if passes.is_empty() => lines.push("_(none recorded)_".to_owned()),
        Some(passes) => {
            for pass in passes {
                let list = |key: &str| pass[key].as_array().into_iter().flatten().map(|item| format!("{} {} ×{}",
                    text(&item["change_type"]), code(&text(&item["node_type"])), item["count"])).collect::<Vec<_>>().join(", ");
                let (removed, added) = (list("removed"), list("added"));
                let mut line = format!("- {}", code(&text(&pass["pass"])));
                if !removed.is_empty() { line.push_str(&format!(": removed {removed}")); }
                if !added.is_empty() { line.push_str(&format!("; added {added}")); }
                lines.push(line);
            }
        }
    }
    lines.push(String::new());
    lines.push("---".to_owned());
    lines.push("Generated by IntentumDiff core. No source text is included: identifiers are placeholders \
(`id1`, …), literals are reduced to type and length, and comments are omitted. Please review before submitting.".to_owned());
    (title, lines)
}

/// Percent-encode a URL query component (RFC 3986 unreserved characters pass through).
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn issue_url(title: &str, body: &str) -> String {
    format!("{}?title={}&body={}", crate::completeness::REPORT_URL, encode(title), encode(body))
}

/// `{title, body_markdown, truncated, issue_url, outline}`. The URL stays within
/// [`MAX_ISSUE_URL_BYTES`]; when the body does not fit, the URL carries a deterministic
/// prefix of whole lines plus a note, `truncated=true`, and `body_markdown` stays complete.
pub(crate) fn render(outline: Value) -> Value {
    let (title, lines) = render_lines(&outline);
    let body = lines.join("\n");
    let full = issue_url(&title, &body);
    let (url, truncated) = if full.len() <= MAX_ISSUE_URL_BYTES {
        (full, false)
    } else {
        let mut kept = lines.len();
        loop {
            let mut prefix = lines[..kept].join("\n");
            prefix.push_str("\n\n");
            prefix.push_str(TRUNCATION_NOTE);
            let url = issue_url(&title, &prefix);
            if url.len() <= MAX_ISSUE_URL_BYTES || kept == 0 {
                break (if url.len() <= MAX_ISSUE_URL_BYTES { url } else { issue_url(&title, TRUNCATION_NOTE) }, true);
            }
            kept -= 1;
        }
    };
    json!({"title": title, "body_markdown": body, "truncated": truncated, "issue_url": url, "outline": outline})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, kind: &str, label: &str, (sl, sc, el, ec): (u64, u64, u64, u64), children: Vec<Value>) -> Value {
        json!({"id":id,"node_type":kind,"label":label,"structural_hash":"h","children":children,
            "position":{"start_line":sl,"start_col":sc,"end_line":el,"end_col":ec}})
    }

    #[test]
    fn placeholders_are_consistent_and_literals_typed() {
        let source = "SECRET = foo(\"hunter2\", 3)\n";
        let tree = node("0", "module", "", (0, 0, 1, 0), vec![
            node("1", "assignment", "", (0, 0, 0, 26), vec![
                node("2", "identifier", "SECRET", (0, 0, 0, 6), vec![]),
                node("3", "call", "", (0, 9, 0, 26), vec![
                    node("4", "identifier", "foo", (0, 9, 0, 12), vec![]),
                    node("5", "argument_list", "", (0, 12, 0, 26), vec![
                        node("6", "string", "hunter2", (0, 13, 0, 22), vec![]),
                        node("7", "integer", "3", (0, 24, 0, 25), vec![]),
                        node("8", "identifier", "SECRET", (0, 24, 0, 25), vec![]),
                    ]),
                ]),
            ]),
        ]);
        let ctx = ReproContext { old_tree: &tree, new_tree: &tree, suppression_trace: &Value::Null };
        let out = outline("python", source, source, &[((0, 26), (0, 26))], false, Some(&ctx));
        let shape = out["regions"][0]["old_shape"].as_str().unwrap();
        assert_eq!(shape, "assignment(id1, call(id2, argument_list(string(9), int, id1)))", "{out}");
        assert_eq!(out["regions"][0]["new_shape"], shape);
        assert_eq!(out["placeholder_count"], 2);
        let text = render(out).to_string();
        assert!(!text.contains("SECRET") && !text.contains("hunter2") && !text.contains("foo"), "{text}");
    }

    #[test]
    fn unsafe_node_kinds_are_withheld() {
        let tree = node("0", "document", "", (0, 0, 1, 0), vec![
            node("1", "mytag", "mytag", (0, 0, 0, 5), vec![node("2", "name", "mytag", (0, 0, 0, 5), vec![])]),
            node("3", "Upper_Kind", "", (0, 0, 0, 5), vec![]),
        ]);
        let ctx = ReproContext { old_tree: &tree, new_tree: &tree, suppression_trace: &Value::Null };
        let out = outline("xml", "mytag\n", "mytag\n", &[((0, 6), (0, 6))], false, Some(&ctx));
        let text = out.to_string();
        assert!(!text.contains("mytag") && !text.contains("Upper_Kind"), "{text}");
    }

    #[test]
    fn url_truncation_is_bounded_deterministic_and_keeps_full_body() {
        let ranges: Vec<_> = (0..64).map(|i| ((i * 10, i * 10 + 5), (i * 10, i * 10 + 5))).collect();
        let source = "x".repeat(700);
        let tree = node("0", "module", "", (0, 0, 0, 700), vec![]);
        let trace = json!((0..80).map(|i| json!({"pass": format!("refine:pass_{i}"),
            "removed": [{"change_type":"REORDER","node_type":"expression_statement","count":2}], "added": []})).collect::<Vec<_>>());
        let ctx = ReproContext { old_tree: &tree, new_tree: &tree, suppression_trace: &trace };
        let build = || render(outline("python", &source, &source, &ranges, true, Some(&ctx)));
        let report = build();
        assert_eq!(report["truncated"], true);
        let url = report["issue_url"].as_str().unwrap();
        assert!(url.len() <= MAX_ISSUE_URL_BYTES, "{}", url.len());
        assert!(url.len() > MAX_ISSUE_URL_BYTES - 400, "uses the budget: {}", url.len());
        assert!(url.contains(&encode(TRUNCATION_NOTE)));
        assert!(issue_url(report["title"].as_str().unwrap(), report["body_markdown"].as_str().unwrap()).len() > MAX_ISSUE_URL_BYTES);
        assert!(report["body_markdown"].as_str().unwrap().contains("refine:pass_79"));
        assert_eq!(build().to_string(), report.to_string(), "deterministic");
        let small = render(outline("python", "a\n", "b\n", &[((0, 2), (0, 2))], false, None));
        assert_eq!(small["truncated"], false);
        assert_eq!(small["issue_url"], issue_url(small["title"].as_str().unwrap(), small["body_markdown"].as_str().unwrap()));
    }
}
