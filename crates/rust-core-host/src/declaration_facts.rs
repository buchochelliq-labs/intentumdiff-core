//! Declaration-level NodeFacts that need language rules: `visibility` and `stub_sentinel`.
//!
//! Both are closed enums, never source text. `visibility` is decided per language from the
//! normalised tree plus the declaration's own header tokens (several parser components prune
//! modifier tokens such as Rust `pub`, C# `private` or the TypeScript accessibility modifier,
//! so the header is re-read from the declaration's source span, the same recovery
//! `operator_tokens` uses for pruned operators):
//!
//! * explicit modifiers where the language has them (Java, C#, Kotlin, Swift, Rust `pub`,
//!   TypeScript accessibility / `#private` / `export`), with each language's documented default
//!   when no modifier is written;
//! * naming conventions only where the language defines them (Python `_x` / `__x` / `__x__`,
//!   Go identifier capitalisation).
//!
//! Anything the rules cannot decide is `"unknown"` — never a guess. Languages without rules
//! here carry no `visibility` key (absent means "not derived").
//!
//! `stub_sentinel` names the placeholder a stub body consists of (`pass`, `ellipsis`,
//! `not_implemented`, `docstring_only`, `todo_macro`, `panic`) and is set only when the
//! node's `body` fact is `"stub"`. A body that consists solely of one recognised language
//! placeholder (Rust `todo!()`, Java `throw new UnsupportedOperationException()`, …) is
//! classified `"stub"` here when no earlier pass decided `body`.
use crate::SemanticNode;
use serde_json::{json, Value};

/// Visibility rules exist for these language ids.
fn visibility_language(language: &str) -> Option<Lang> {
    Some(match language {
        "python" => Lang::Python,
        "go" => Lang::Go,
        "java" => Lang::Java,
        "csharp" => Lang::CSharp,
        "kotlin" => Lang::Kotlin,
        "swift" => Lang::Swift,
        "rust" => Lang::Rust,
        "typescript" | "tsx" | "javascript" | "jsx" => Lang::Script,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    Python,
    Go,
    Java,
    CSharp,
    Kotlin,
    Swift,
    Rust,
    Script,
}

/// A declaration that carries an access level: functions/methods, classes and type-like
/// declarations.
fn is_declaration_type(node_type: &str) -> bool {
    crate::is_function_entity_type(node_type)
        || crate::is_class_entity_type(node_type)
        || matches!(
            node_type,
            "struct_item"
                | "enum_item"
                | "trait_item"
                | "interface_declaration"
                | "enum_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "object_declaration"
                | "protocol_declaration"
                | "type_declaration"
                | "abstract_class_declaration"
        )
}

/// Fill `visibility` and `stub_sentinel` facts in place. `source` is the text the tree was
/// parsed from (may be empty, in which case only tree evidence is used).
pub(crate) fn enrich_declaration_facts(tree: &mut SemanticNode, source: &str, language: &str) {
    let lang = visibility_language(language);
    let starts = line_starts(source);
    let is_module = tree.children.iter().any(|c| {
        matches!(c.node_type.as_str(), "export_statement" | "import_statement")
    });
    let ctx = Ctx { lang, source, starts: &starts, is_module };
    walk(tree, &ctx, &[]);
}

struct Ctx<'a> {
    lang: Option<Lang>,
    source: &'a str,
    starts: &'a [usize],
    is_module: bool,
}

/// `ancestors` holds the node types (and, for Rust impls, whether the impl is a trait impl)
/// from the root down to the parent.
fn walk(node: &mut SemanticNode, ctx: &Ctx, ancestors: &[(String, bool)]) {
    if is_declaration_type(&node.node_type) {
        let mut additions = serde_json::Map::new();
        if let Some(lang) = ctx.lang {
            additions.insert("visibility".to_owned(), json!(visibility(node, lang, ctx, ancestors)));
        }
        if crate::is_function_entity_type(&node.node_type) || crate::is_class_entity_type(&node.node_type) {
            let body = node.facts.as_ref().and_then(|f| f.get("body")).and_then(Value::as_str).map(str::to_owned);
            match body.as_deref() {
                Some("stub") => {
                    if let Some(sentinel) = stub_sentinel(node) {
                        additions.insert("stub_sentinel".to_owned(), json!(sentinel));
                    }
                }
                None if crate::is_function_entity_type(&node.node_type) => {
                    if let Some(sentinel) = placeholder_body(node, ctx.lang) {
                        additions.insert("body".to_owned(), json!("stub"));
                        additions.insert("stub_sentinel".to_owned(), json!(sentinel));
                    }
                }
                _ => {}
            }
        }
        if !additions.is_empty() {
            crate::merge_facts(&mut node.facts, Value::Object(additions));
        }
    }
    let trait_impl = node.node_type == "impl_item" && rust_impl_is_trait_impl(node, ctx);
    let mut next = ancestors.to_vec();
    next.push((node.node_type.clone(), trait_impl));
    for child in &mut node.children {
        walk(child, ctx, &next);
    }
}

fn parent_type<'a>(ancestors: &'a [(String, bool)]) -> &'a str {
    ancestors.last().map(|(t, _)| t.as_str()).unwrap_or("")
}

fn inside(ancestors: &[(String, bool)], types: &[&str]) -> bool {
    ancestors.iter().any(|(t, _)| types.contains(&t.as_str()))
}

/// The nearest enclosing declaration-or-body container, ignoring pure body wrappers.
fn enclosing_container<'a>(ancestors: &'a [(String, bool)]) -> Option<&'a (String, bool)> {
    ancestors.iter().rev().find(|(t, _)| {
        !matches!(
            t.as_str(),
            "class_body" | "declaration_list" | "interface_body" | "enum_body" | "block"
                | "field_declaration_list" | "statement_block" | "enum_body_declarations"
        )
    })
}

fn visibility(node: &SemanticNode, lang: Lang, ctx: &Ctx, ancestors: &[(String, bool)]) -> &'static str {
    let name = declared_name(node);
    match lang {
        Lang::Python => python_visibility(&name),
        Lang::Go => go_visibility(&name),
        Lang::Java => {
            let words = header_words(node, ctx);
            if let Some(v) = words.as_deref().and_then(|w| first_modifier(w, &[("public", "public"), ("private", "private"), ("protected", "protected")])) {
                return v;
            }
            if inside(ancestors, &["interface_declaration", "interface_body", "annotation_type_declaration"]) {
                // Interface members are implicitly public.
                "public"
            } else if words.is_none() && !node.children.iter().any(|c| c.node_type == "modifiers") {
                "unknown"
            } else {
                // No access modifier: package-private.
                "internal"
            }
        }
        Lang::CSharp => {
            let Some(words) = header_words(node, ctx) else { return "unknown" };
            let has = |w: &str| words.iter().any(|x| x == w);
            if has("protected") {
                // `protected internal` / `private protected` both admit derived types.
                return "protected";
            }
            if has("public") {
                return "public";
            }
            if has("private") {
                return "private";
            }
            if has("internal") || has("file") {
                return "internal";
            }
            match enclosing_container(ancestors).map(|(t, _)| t.as_str()) {
                Some("interface_declaration") => "public",
                Some(
                    "class_declaration" | "struct_declaration" | "record_declaration"
                    | "record_struct_declaration",
                ) => "private",
                // Top-level and namespace-level types default to internal.
                Some("compilation_unit" | "namespace_declaration" | "file_scoped_namespace_declaration")
                | None => "internal",
                _ => "unknown",
            }
        }
        Lang::Kotlin => {
            let Some(words) = header_words(node, ctx) else { return "unknown" };
            first_modifier(&words, &[("public", "public"), ("private", "private"),
                ("protected", "protected"), ("internal", "internal")])
                // Kotlin's default visibility is public.
                .unwrap_or("public")
        }
        Lang::Swift => {
            let Some(words) = header_words(node, ctx) else { return "unknown" };
            first_modifier(&words, &[("open", "public"), ("public", "public"), ("internal", "internal"),
                ("fileprivate", "private"), ("private", "private")])
                // Swift's default access level is internal.
                .unwrap_or("internal")
        }
        Lang::Rust => {
            // Trait items and trait-impl members take the trait's visibility, which this
            // declaration does not state.
            if inside(ancestors, &["trait_item"]) || ancestors.iter().any(|(t, is_trait)| t == "impl_item" && *is_trait) {
                return "unknown";
            }
            match rust_visibility(node, ctx) {
                Some(v) => v,
                None => "unknown",
            }
        }
        Lang::Script => {
            if name.starts_with('#') {
                return "private";
            }
            let parent = parent_type(ancestors);
            if matches!(parent, "class_body") {
                let Some(words) = header_words(node, ctx) else { return "unknown" };
                return first_modifier(&words, &[("private", "private"), ("protected", "protected"), ("public", "public")])
                    // Class members are public unless marked otherwise.
                    .unwrap_or("public");
            }
            if parent == "export_statement" {
                return "public";
            }
            if matches!(parent, "program" | "") && ancestors.len() <= 1 {
                // A non-exported top-level declaration is module-private only when the file
                // is a module; a script's top level is global, which we do not guess at.
                return if ctx.is_module { "private" } else { "unknown" };
            }
            "unknown"
        }
    }
}

fn python_visibility(name: &str) -> &'static str {
    if name.is_empty() {
        "unknown"
    } else if name.len() > 4 && name.starts_with("__") && name.ends_with("__") {
        "special"
    } else if name.starts_with("__") {
        // Name-mangled: class-private by convention.
        "private"
    } else if name.starts_with('_') {
        // PEP 8 "weak internal use" indicator.
        "internal"
    } else {
        "public"
    }
}

fn go_visibility(name: &str) -> &'static str {
    match name.chars().next() {
        Some(c) if c.is_uppercase() => "public",
        // Unexported identifiers are visible within their package.
        Some(c) if c.is_alphabetic() || c == '_' => "internal",
        _ => "unknown",
    }
}

fn first_modifier(words: &[String], table: &[(&str, &'static str)]) -> Option<&'static str> {
    words.iter().find_map(|w| table.iter().find(|(k, _)| k == w).map(|(_, v)| *v))
}

/// The declared name: the first identifier-like child's label, else the node label.
fn declared_name(node: &SemanticNode) -> String {
    node.children
        .iter()
        .find(|c| is_name_type(&c.node_type))
        .map(|c| c.label.clone())
        .unwrap_or_else(|| node.label.clone())
}

fn is_name_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "identifier" | "type_identifier" | "property_identifier" | "private_property_identifier"
            | "field_identifier" | "simple_identifier" | "name"
    )
}

/// Declaration keywords that end the modifier prefix.
const DECL_KEYWORDS: &[&str] = &[
    "fn", "fun", "func", "def", "function", "class", "struct", "enum", "trait", "interface",
    "record", "object", "protocol", "type", "void", "impl", "extension", "init", "constructor",
];

/// The modifier words in the declaration header: words from the node's start up to the
/// declared name, a declaration keyword, or the first `(`/`{`/`=`/`;` — skipping string
/// literals and attribute/annotation arguments. `None` when no source span is available.
fn header_words(node: &SemanticNode, ctx: &Ctx) -> Option<Vec<String>> {
    if ctx.source.is_empty() {
        // Tree-only evidence: a retained `modifiers` child (Java) carries its keywords.
        let modifiers = node.children.iter().find(|c| c.node_type == "modifiers")?;
        return Some(modifiers.label.split_whitespace().map(str::to_owned).collect());
    }
    let start = byte_offset(ctx.starts, ctx.source, node.position.start_line, node.position.start_col)?;
    let end = byte_offset(ctx.starts, ctx.source, node.position.end_line, node.position.end_col)
        .unwrap_or(ctx.source.len())
        .min(start + 1024)
        .max(start);
    let mut end = end.min(ctx.source.len());
    while !ctx.source.is_char_boundary(end) {
        end -= 1;
    }
    let text = &ctx.source[start..end];
    let name = declared_name(node);
    let name = name.trim_start_matches('#');
    let mut words = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' | b'\'' | b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'[' => i = skip_balanced(bytes, i, b'[', b']'),
            b'@' => {
                // Annotation: skip the name and any argument list.
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.') {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'(' {
                    i = skip_balanced(bytes, i, b'(', b')');
                }
            }
            b'(' => {
                // Rust `pub(crate)`: keep the restriction as `pub(` + word.
                if words.last().map(String::as_str) == Some("pub") {
                    let close = skip_balanced(bytes, i, b'(', b')');
                    let inner = String::from_utf8_lossy(&bytes[i + 1..close.saturating_sub(1).max(i + 1)]).trim().to_owned();
                    words.pop();
                    words.push(format!("pub({inner})"));
                    i = close;
                } else {
                    break;
                }
            }
            b'{' | b'=' | b';' | b':' | b'<' => break,
            _ if b.is_ascii_alphabetic() || b == b'_' => {
                let s = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &text[s..i];
                if (!name.is_empty() && word == name) || DECL_KEYWORDS.contains(&word) {
                    break;
                }
                words.push(word.to_owned());
            }
            _ => i += 1,
        }
    }
    Some(words)
}

fn skip_balanced(bytes: &[u8], mut i: usize, open: u8, close: u8) -> usize {
    let mut depth = 0usize;
    while i < bytes.len() {
        if bytes[i] == open {
            depth += 1;
        } else if bytes[i] == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    i
}

fn rust_visibility(node: &SemanticNode, ctx: &Ctx) -> Option<&'static str> {
    if let Some(vis) = node.children.iter().find(|c| c.node_type == "visibility_modifier") {
        return Some(rust_pub_kind(vis.label.trim()));
    }
    let words = header_words(node, ctx)?;
    Some(words.iter().find(|w| w.starts_with("pub")).map(|w| rust_pub_kind(w)).unwrap_or("private"))
}

fn rust_pub_kind(word: &str) -> &'static str {
    let compact: String = word.chars().filter(|c| !c.is_whitespace()).collect();
    match compact.as_str() {
        "pub" => "public",
        "pub(self)" => "private",
        w if w.starts_with("pub(") => "internal",
        _ => "private",
    }
}

/// `impl Trait for Type` (members inherit the trait's visibility) vs an inherent `impl Type`.
fn rust_impl_is_trait_impl(node: &SemanticNode, ctx: &Ctx) -> bool {
    if ctx.lang != Some(Lang::Rust) || ctx.source.is_empty() {
        return false;
    }
    let Some(start) = byte_offset(ctx.starts, ctx.source, node.position.start_line, node.position.start_col) else {
        return false;
    };
    let header = ctx.source[start..].split('{').next().unwrap_or("");
    header.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "for")
}

/// The sentinel for a body the earlier passes already classified as `stub` (Python's
/// trivial statements). The first non-docstring statement decides; only docstrings →
/// `docstring_only`.
fn stub_sentinel(node: &SemanticNode) -> Option<&'static str> {
    let block = node.children.iter().find(|c| c.node_type == "block")?;
    let mut saw_docstring = false;
    for stmt in &block.children {
        match stmt.node_type.as_str() {
            "pass_statement" => return Some("pass"),
            "raise_statement" => {
                if subtree_has_label(stmt, &["NotImplementedError", "NotImplemented"]) {
                    return Some("not_implemented");
                }
            }
            "expression_statement" => {
                if stmt.children.iter().any(|c| c.node_type == "ellipsis") {
                    return Some("ellipsis");
                }
                if stmt.children.iter().all(|c| matches!(c.node_type.as_str(), "string" | "concatenated_string")) {
                    saw_docstring = true;
                }
            }
            "ellipsis" => return Some("ellipsis"),
            "string" | "concatenated_string" => saw_docstring = true,
            _ => {}
        }
    }
    saw_docstring.then_some("docstring_only")
}

fn subtree_has_label(node: &SemanticNode, labels: &[&str]) -> bool {
    labels.contains(&node.label.as_str()) || node.children.iter().any(|c| subtree_has_label(c, labels))
}

/// A body consisting solely of one recognised placeholder statement, per language.
fn placeholder_body(node: &SemanticNode, lang: Option<Lang>) -> Option<&'static str> {
    let lang = lang?;
    if matches!(lang, Lang::Kotlin | Lang::Swift) {
        // The Kotlin/Swift components keep body statements as direct children after the
        // `signature` line, labelled with their call text.
        let body: Vec<&SemanticNode> = node.children.iter().filter(|c| c.node_type != "signature").collect();
        if let [only] = body.as_slice() {
            let call = matches!(only.node_type.as_str(), "call_expression" | "function_call_expression");
            if call && lang == Lang::Kotlin && (only.label == "TODO" || only.label.starts_with("TODO(")) {
                return Some("todo_macro");
            }
            if call && lang == Lang::Swift && (only.label == "fatalError" || only.label.starts_with("fatalError(")) {
                return Some("panic");
            }
        }
    }
    let block = node.children.iter().find(|c| {
        matches!(c.node_type.as_str(), "block" | "function_body" | "statement_block" | "statements")
    })?;
    // Unwrap single-statement wrappers (Go `statement_list`, expression statements).
    let mut stmt = sole_child(block)?;
    loop {
        match stmt.node_type.as_str() {
            "statement_list" | "expression_statement" => stmt = sole_child(stmt)?,
            _ => break,
        }
    }
    match (lang, stmt.node_type.as_str()) {
        (Lang::Rust, "macro_invocation") => match stmt.label.as_str() {
            "todo" | "unimplemented" => Some("todo_macro"),
            "panic" => Some("panic"),
            _ => None,
        },
        (Lang::Java | Lang::CSharp, "throw_statement") => {
            subtree_has_label(stmt, &["UnsupportedOperationException", "NotImplementedException"])
                .then_some("not_implemented")
        }
        (Lang::Go, "call_expression") if stmt.label == "panic" => Some("panic"),
        _ => None,
    }
}

fn sole_child(node: &SemanticNode) -> Option<&SemanticNode> {
    match node.children.as_slice() {
        [only] => Some(only),
        _ => None,
    }
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn byte_offset(starts: &[usize], source: &str, line: u32, col: u32) -> Option<usize> {
    let offset = starts.get(line as usize)? + col as usize;
    (offset <= source.len() && source.is_char_boundary(offset)).then_some(offset)
}

#[cfg(all(test, feature = "tier-c-wasm"))]
#[path = "declaration_facts_source_tests.rs"]
mod declaration_facts_source_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_and_modifier_rules_are_closed_enums() {
        assert_eq!(python_visibility("__init__"), "special");
        assert_eq!(python_visibility("____"), "private");
        assert_eq!(python_visibility("_x"), "internal");
        assert_eq!(python_visibility("x"), "public");
        assert_eq!(go_visibility("Run"), "public");
        assert_eq!(go_visibility("run"), "internal");
        assert_eq!(rust_pub_kind("pub"), "public");
        assert_eq!(rust_pub_kind("pub( crate )"), "internal");
        assert_eq!(rust_pub_kind("pub(in crate::a)"), "internal");
        assert_eq!(rust_pub_kind("pub(self)"), "private");
    }
}
