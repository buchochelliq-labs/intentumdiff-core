//! Source-backed updates for parser payload omitted from the exported node projection.
use crate::*;

fn local_hash_delta(old: &SemanticNode, new: &SemanticNode) -> bool {
    old.structural_hash != new.structural_hash
        && old.node_type == new.node_type
        && old.label == new.label
        && old.children.len() == new.children.len()
        && old
            .children
            .iter()
            .zip(&new.children)
            .all(|(a, b)| a.node_type == b.node_type)
}

/// Compare only source owned by this node; projected children cannot
/// supply a false payload delta or mask a missing type/table-header/value token.
fn gaps(node: &SemanticNode, source: &str) -> Option<Vec<String>> {
    let (start, end) = rust_source_span_offsets(source, &node.position)?;
    let mut cursor = start;
    let mut result = Vec::new();
    for child in &node.children {
        let (cs, ce) = rust_source_span_offsets(source, &child.position)?;
        if cs < cursor || ce < cs || ce > end {
            return None;
        }
        result.push(source.get(cursor..cs)?.to_owned());
        cursor = ce;
    }
    result.push(source.get(cursor..end)?.to_owned());
    Some(result)
}

fn gap_tokens(node: &SemanticNode, source: &str, language: &str) -> Option<Vec<Vec<String>>> {
    gaps(node, source)?
        .iter()
        .map(|gap| tokens(gap, language))
        .collect()
}

pub(crate) fn changed(
    old: &SemanticNode,
    new: &SemanticNode,
    old_source: &str,
    new_source: &str,
    language: &str,
) -> bool {
    if !local_hash_delta(old, new) || !configured_language(language) {
        return false;
    }
    let (Some(old_gaps), Some(new_gaps)) = (gaps(old, old_source), gaps(new, new_source)) else {
        return false; // Invalid spans cannot supply source-owned evidence.
    };
    match (
        gap_tokens(old, old_source, language),
        gap_tokens(new, new_source, language),
    ) {
        (Some(a), Some(b)) => a != b,
        // Declining interpretation is not proof of equivalence. Retain the
        // validated raw own-source delta instead of silently losing the edit.
        _ => old_gaps != new_gaps,
    }
}

pub(crate) fn annotate(changes: &mut [ChangeDraft<'_>], old: &str, new: &str, language: &str) {
    for change in changes {
        let (Some(a), Some(b)) = (change.old_node, change.new_node) else {
            continue;
        };
        if !changed(a, b, old, new, language) {
            continue;
        }
        let source = |node: &SemanticNode, text: &str| {
            rust_source_span_offsets(text, &node.position)
                .and_then(|(s, e)| text.get(s..e))
                .unwrap_or("")
                .to_owned()
        };
        change.text_diff = Some(format!("{} -> {}", source(a, old), source(b, new)));
        change.description = format!(
            "Parser payload changed in {}({:?}); source evidence retained",
            a.node_type, a.label
        );
    }
}

/// A normalized projection is insufficient when a node's own hash changed while
/// its exported label and children did not. Sources may discharge that ambiguity
/// only when their own token streams compare equal, with literals kept intact.
pub(crate) fn trees_equivalent(
    old: &SemanticNode,
    new: &SemanticNode,
    sources: Option<(&str, &str, &str)>,
) -> bool {
    // Native routes already trust equal parser hashes (for example unordered
    // mapping representations). Preserve that existing proof independently of
    // exported child order; the source-less API still checks its projection.
    if sources.is_some() && old.structural_hash == new.structural_hash {
        return true;
    }
    if whitespace_normalized_tree_hash(old) != whitespace_normalized_tree_hash(new) {
        return false;
    }
    if old.structural_hash == new.structural_hash {
        return true;
    }
    // Source-backed opaque update generation is scoped to configured profiles.
    // Other source-aware routes retain their existing normalized equivalence;
    // tightening their certificate requires corresponding producer support.
    if sources.is_some_and(|(_, _, language)| !configured_language(language)) {
        return true;
    }
    fn explained(
        old: &SemanticNode,
        new: &SemanticNode,
        sources: Option<(&str, &str, &str)>,
    ) -> bool {
        if old.structural_hash != new.structural_hash {
            if let Some((a, b, language)) = sources {
                match (gap_tokens(old, a, language), gap_tokens(new, b, language)) {
                    (Some(a), Some(b)) if a == b => {}
                    _ => return false,
                }
            } else if old.label == new.label {
                return false;
            }
        }
        old.children
            .iter()
            .zip(&new.children)
            .all(|(a, b)| explained(a, b, sources))
    }
    explained(old, new, sources)
}

/// A small lexical comparison, not a new language parser. Strings and escaped
/// comment markers stay intact; only the grammar's comments and layout disappear.
/// Unsupported multiline/backtick literals decline proof rather than guess.
fn configured_language(language: &str) -> bool {
    matches!(
        language,
        "make"
            | "makefile"
            | "proto"
            | "puppet"
            | "toml"
            | "python"
            | "javascript"
            | "typescript"
            | "tsx"
            | "csharp"
            | "c"
            | "cpp"
            | "go"
            | "java"
            | "rust"
            | "dart"
            | "kotlin"
            | "scala"
    )
}

fn tokens(source: &str, language: &str) -> Option<Vec<String>> {
    use crate::operator_tokens::{comment_styles, CommentStyle};
    // Equivalence needs a known lexical dialect. Other grammars still retain
    // their hash evidence, but this comparison must not guess at their comments.
    if !configured_language(language) {
        return None;
    }
    let mut result = Vec::new();
    let mut i = 0;
    let make = matches!(language, "make" | "makefile");
    // Make quotes are ordinary value bytes, and # still starts a comment in
    // them. Nested variable/function expansion has separate comment rules.
    if make && (source.contains("$(") || source.contains("${")) {
        return None;
    }
    let mut make_value = false;
    let mut value_started = false;
    while i < source.len() {
        let rest = &source[i..];
        let ch = rest.chars().next()?;
        if rest.starts_with("R\"") || rest.starts_with("r\"") || rest.starts_with("r#") {
            return None;
        }
        if (language == "python"
            && [
                "r'", "R'", "b'", "B'", "f'", "F'", "b\"", "B\"", "f\"", "F\"",
            ]
            .iter()
            .any(|p| rest.starts_with(p)))
            || (language == "csharp"
                && ["@\"", "$\"", "$@\"", "@$\""]
                    .iter()
                    .any(|p| rest.starts_with(p)))
        {
            return None;
        }
        if ch.is_whitespace() {
            let len = rest
                .chars()
                .take_while(|c| c.is_whitespace())
                .map(char::len_utf8)
                .sum::<usize>();
            let whitespace = &rest[..len];
            if make && make_value && value_started {
                result.push(format!("layout:{whitespace}"));
            }
            if whitespace.contains('\n') {
                make_value = false;
                value_started = false;
            }
            i += len;
            continue;
        }
        let mut comment = None;
        for style in comment_styles(language) {
            comment = match style {
                CommentStyle::Hash if rest.starts_with('#') => {
                    Some(rest.find('\n').unwrap_or(rest.len()))
                }
                CommentStyle::DashDash if rest.starts_with("--") => {
                    Some(rest.find('\n').unwrap_or(rest.len()))
                }
                CommentStyle::Slash if rest.starts_with("//") => {
                    Some(rest.find('\n').unwrap_or(rest.len()))
                }
                CommentStyle::Slash if rest.starts_with("/*") => {
                    let end = rest.find("*/")? + 2;
                    if rest[2..end - 2].contains("/*") {
                        return None;
                    }
                    Some(end)
                }
                _ => None,
            };
            if comment.is_some() {
                break;
            }
        }
        if let Some(len) = comment {
            i += len;
            continue;
        }
        if ch == '/' && matches!(language, "javascript" | "typescript" | "tsx" | "puppet") {
            return None; // Distinguishing a regex literal from division needs the grammar.
        }
        if ch == '`' || rest.starts_with("\"\"\"") || rest.starts_with("'''") {
            return None;
        }
        let start = i;
        if !make && matches!(ch, '\'' | '"') {
            i += 1;
            let mut closed = false;
            while i < source.len() {
                let c = source[i..].chars().next()?;
                i += c.len_utf8();
                if c == '\\' && !(language == "toml" && ch == '\'') {
                    i += source[i..].chars().next()?.len_utf8();
                } else if c == ch {
                    closed = true;
                    break;
                }
            }
            if !closed {
                return None;
            }
        } else if ch == '\\' {
            i += 1;
            i += source[i..].chars().next()?.len_utf8();
        } else if ch.is_alphanumeric() || ch == '_' {
            i += rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .map(char::len_utf8)
                .sum::<usize>();
            if !make && source[i..].starts_with(['\'', '"']) {
                return None; // A prefixed string needs its grammar's complete decoder.
            }
        } else {
            i += ch.len_utf8();
        }
        let raw = &source[start..i];
        let quoted = !make && raw.starts_with(['\'', '"']);
        let token = if quoted {
            literal_token(raw, language)?
        } else {
            rust_literal_invariance_language(language)
                .then(|| canonical_integer_literal_for_invariance(raw))
                .flatten()
                .map(|value| format!("integer:{value}"))
                .unwrap_or_else(|| format!("token:{raw}"))
        };
        result.push(token);
        if make && raw == "=" {
            make_value = true;
            value_started = false;
        } else if make_value {
            value_started = true;
        }
    }
    Some(result)
}

fn literal_token(raw: &str, language: &str) -> Option<String> {
    let quote = raw.chars().next()?;
    let inner = raw.get(1..raw.len().checked_sub(1)?)?;
    // Only escape-free plain literals are a lexical equivalence proof here.
    // Keep other fully lexed strings atomic and exact: declining interpretation
    // must not turn differing opaque payload into "no source delta".
    if inner.contains('\\') || (language == "puppet" && quote == '"' && inner.contains('$')) {
        return Some(format!("raw_literal:{raw}"));
    }
    // These profiles explicitly allow either quote spelling for plain strings.
    // In C-family languages a character and a string remain distinct tokens.
    let kind = if matches!(
        language,
        "proto" | "toml" | "puppet" | "python" | "javascript" | "typescript" | "tsx"
    ) {
        "string".to_owned()
    } else {
        format!("literal:{quote}")
    };
    Some(format!("{kind}:{inner}"))
}
