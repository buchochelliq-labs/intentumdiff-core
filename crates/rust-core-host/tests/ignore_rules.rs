use intentumdiff_rust_core::ignore_rules::{IgnoreFile, IgnoreRules};
#[test]
fn excluded_parent_cannot_be_reincluded_by_child_rule() {
    let rules = IgnoreRules::new(&[IgnoreFile {directory: "".into(), content: "build/\n!build/keep.py\n".into()}]).unwrap();
    assert!(rules.is_ignored("build/keep.py", false).unwrap());
}

#[test]
fn source_judged_ignore_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/ignore_rules.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let files: Vec<IgnoreFile> = serde_json::from_value(case["files"].clone()).unwrap();
        let rules = IgnoreRules::new(&files).unwrap();
        assert_eq!(rules.is_ignored(case["path"].as_str().unwrap(), false).unwrap(), case["expected"].as_bool().unwrap(), "{}", case["name"]);
    }
}
#[test]
fn invalid_paths_are_errors() {
    let rules = IgnoreRules::new(&[]).unwrap();
    for path in ["../escape", "/absolute", "a\\b", "a//b", ""] { assert!(rules.is_ignored(path, false).is_err()); }
}
