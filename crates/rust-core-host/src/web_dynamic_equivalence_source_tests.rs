//! Source-judged regressions for #270 (Perl spacing), #272 (Astro/Svelte embedded JS quote
//! spelling) and #273 (CSS comment beside a property name): equal semantics are not changes,
//! and genuine edits next to them stay meaningful.
use crate::*;
use serde_json::Value;

fn diff(language: &str, filename: &str, old: &str, new: &str) -> Value {
    let wasm = std::env::var("INTENTUMDIFF_TEST_WASM_DIR").expect("staged parser components");
    let parser = std::path::Path::new(&wasm).join(format!("{language}_parser.wasm"));
    assert!(parser.is_file(), "parser missing: {}", parser.display());
    native_wasm_single_diff(language, parser.to_str().unwrap(), old, new, filename, filename, "{}", None)
        .unwrap_or_else(|e| panic!("{language}: {e}"))
}

fn assert_preserved(language: &str, filename: &str, old: &str, new: &str) {
    let d = diff(language, filename, old, new);
    assert_eq!(d["has_semantic_changes"], false, "{language}: {d:#}");
    assert_eq!(d["is_style_only"], true, "{language}: equal semantics are style-only evidence: {d:#}");
}

fn assert_meaningful(language: &str, filename: &str, old: &str, new: &str) {
    let d = diff(language, filename, old, new);
    assert_eq!(d["has_semantic_changes"], true, "{language}: {d:#}");
}

#[test]
fn perl_assignment_spacing_is_not_a_semantic_change() {
    assert_preserved("perl", "a.pl", "my $x=1+2; print $x;\n", "my $x = 1 + 2; print $x;\n");
    assert_meaningful("perl", "a.pl", "my $x=1+2; print $x;\n", "my $x=1+3; print $x;\n");
    assert_meaningful("perl", "a.pl", "my $x=1+2; print $x;\n", "my $x=1*2; print $x;\n");
}

#[test]
fn embedded_javascript_quote_spelling_is_not_a_semantic_change() {
    assert_preserved("astro", "a.astro", "---\nconst title = 'Ready';\n---\n<h1>{title}</h1>\n",
        "---\nconst title = \"Ready\";\n---\n<h1>{title}</h1>\n");
    assert_meaningful("astro", "a.astro", "---\nconst title = 'Ready';\n---\n<h1>{title}</h1>\n",
        "---\nconst title = \"Steady\";\n---\n<h1>{title}</h1>\n");
    assert_preserved("svelte", "a.svelte", "<script>const title = 'Ready';</script>\n<h1>{title}</h1>\n",
        "<script>const title = \"Ready\";</script>\n<h1>{title}</h1>\n");
    assert_meaningful("svelte", "a.svelte", "<script>const title = 'Ready';</script>\n<h1>{title}</h1>\n",
        "<script>const title = 'Ready!';</script>\n<h1>{title}</h1>\n");
}

#[test]
fn css_comment_is_not_part_of_the_property_name() {
    assert_preserved("css", "a.css", ".card { color: red; }\n", ".card { /* note */ color: red; }\n");
    assert_meaningful("css", "a.css", ".card { color: red; }\n", ".card { /* note */ color: blue; }\n");
    assert_meaningful("css", "a.css", ".card { color: red; }\n", ".card { /* note */ background: red; }\n");
}
