//! `SemanticDiff.content_class` — the coarse kind of content a served diff reviewed:
//! `code` | `docs` | `config` | `data` | `text`.
//!
//! Derived only from what the engine already decided about the file: the routed `language`
//! (filename/content detection picked it) and, for the generic text route, nothing more.
//! A closed table, so every binding reads the same class instead of re-deriving one from
//! language names (intentumdiff-vscode#133). It states WHAT was reviewed, never a risk.
use serde_json::{json, Value};

/// The content class for a routed language id.
pub fn content_class(language: &str) -> &'static str {
    match language {
        // Prose and document markup.
        "markdown" | "mdx" | "asciidoc" | "latex" | "rst" | "org" => "docs",
        // Settings, build/deploy declarations and workflow definitions.
        "yaml" | "toml" | "ini" | "hcl" | "dockerfile" | "puppet" | "gitignore" | "gomod"
        | "databricks" | "databricks-workflow" | "adf" | "dbt-config" | "dbt-packages"
        | "dbt-yaml" | "properties" | "editorconfig" => "config",
        // Serialisation / interchange formats and translation catalogues.
        "json" | "xml" | "csv" | "tsv" | "po" => "data",
        // No grammar: the line/character text review.
        "generic" | "text" | "plaintext" | "" => "text",
        _ => "code",
    }
}

/// Stamp `content_class` on a served diff from its `language` field (idempotent).
pub(crate) fn attach(diff: &mut Value) {
    let Some(object) = diff.as_object_mut() else { return };
    let language = object.get("language").and_then(Value::as_str).unwrap_or("").to_owned();
    let mut class = content_class(&language);
    // The generic text route reviews Markdown files with its Markdown line rules
    // (`native_generic_text_diff`); classify them by that same filename decision.
    if class == "text" {
        let markdown = ["old_filename", "new_filename"].iter().any(|key| {
            object.get(*key).and_then(Value::as_str).is_some_and(|name| {
                let lower = name.to_lowercase();
                lower.ends_with(".md") || lower.ends_with(".markdown")
            })
        });
        if markdown {
            class = "docs";
        }
    }
    object.insert("content_class".to_owned(), json!(class));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_maps_to_a_closed_class() {
        for (language, class) in [("python", "code"), ("rust", "code"), ("html", "code"), ("sql", "code"),
            ("markdown", "docs"), ("latex", "docs"), ("yaml", "config"), ("dockerfile", "config"),
            ("json", "data"), ("xml", "data"), ("generic", "text")] {
            assert_eq!(content_class(language), class, "{language}");
        }
        let mut diff = json!({"language": "toml", "changes": []});
        attach(&mut diff);
        attach(&mut diff);
        assert_eq!(diff["content_class"], "config");
    }

    #[test]
    fn text_routes_serve_content_class() {
        let text = crate::review_text("a\n", "b\n", "notes.txt", "notes.txt").unwrap();
        assert_eq!(text["content_class"], "text", "{text}");
        let markdown = crate::review_text("# A\n", "# B\n", "README.md", "README.md").unwrap();
        assert_eq!(markdown["content_class"], "docs", "{markdown}");
        let typed = crate::api::review_text("a\n", "b\n", "a.txt", "a.txt").unwrap();
        assert_eq!(typed.content_class.as_deref(), Some("text"));
        let fallback = crate::source_fallback::source_fallback_diff_impl("a", "b", "a.toml", "a.toml", "toml", "parse_errors").unwrap();
        assert_eq!(fallback["content_class"], "config", "{fallback}");
        let request = json!({"language":"generic","old_source":"old","new_source":"new",
            "old_filename":"a.txt","new_filename":"a.txt","finalized":{"changes":[],"change_groups":[]}});
        let routed = crate::routed_review::complete(&request).unwrap();
        assert_eq!(routed["content_class"], "text", "{routed}");
    }

    #[cfg(feature = "tier-c-wasm")]
    #[test]
    fn wasm_routes_serve_content_class() {
        let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
        for (language, component, file, old, new, class) in [
            ("python", "python_parser.wasm", "a.py", "x = 1\n", "x = 2\n", "code"),
            ("json", "json_parser.wasm", "a.json", "{\"a\": 1}\n", "{\"a\": 2}\n", "data"),
            ("yaml", "yaml_parser.wasm", "a.yaml", "a: 1\n", "a: 2\n", "config"),
            ("markdown", "markdown_parser.wasm", "a.md", "# A\n\ntext\n", "# A\n\nmore text\n", "docs"),
            // File lifecycle routes (an added file) carry the class too.
            ("yaml", "yaml_parser.wasm", "b.yaml", "", "a: 1\n", "config"),
        ] {
            let parser = std::path::Path::new(&wasm).join(component);
            let diff = crate::native_wasm_single_diff(language, parser.to_str().unwrap(), old, new, file, file, "{}", None)
                .unwrap_or_else(|e| panic!("{language}: {e}"));
            assert_eq!(diff["content_class"], class, "{language}: {diff}");
        }
    }
}
