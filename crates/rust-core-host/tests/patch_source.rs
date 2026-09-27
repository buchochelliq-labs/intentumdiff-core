use intentumdiff_rust_core::patch_source::reconstruct;

#[test]
fn insertion_after_first_line_uses_zero_length_range_anchor() {
    let patch = "--- a/x.txt\n+++ b/x.txt\n@@ -1,0 +2 @@\n+new\n";
    let out = reconstruct(patch, Some("one\ntwo\n"), None, false).unwrap();
    assert_eq!(out.new_content, "one\nnew\ntwo\n");
}

#[test]
fn mismatched_original_is_an_error() {
    let patch = "--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-old\n+new\n";
    assert!(reconstruct(patch, Some("unrelated\n"), None, false).is_err());
}

#[test]
fn source_judged_patch_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/patch_source.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result = reconstruct(case["patch"].as_str().unwrap(),case["original"].as_str(),case["filename"].as_str(),case["require_complete"].as_bool().unwrap_or(false));
        if let Some(error) = case["error"].as_str() { assert!(result.unwrap_err().contains(error),"{}",case["name"]); }
        else {
            let out = serde_json::to_value(result.unwrap()).unwrap();
            for (key,value) in case["expected"].as_object().unwrap() { assert_eq!(&out[key], value,"{} {key}",case["name"]); }
            assert_eq!(out["warning"].is_string(), out["scope"] == "excerpt");
        }
    }
}
