//! Test-only FullParse guest. All tree processing comes from imported host-utils.
wit_bindgen::generate!({ path: "../../../wit/plugin.wit", world: "parser-plugin" });

use exports::intentdiff::plugin::parser::{ExamplePair, Guest, LanguageInfoRecord, ParserMode};
use intentdiff::plugin::host_utils;
use serde_json::Value;

struct Probe;

impl Guest for Probe {
    fn get_parser_mode() -> ParserMode {
        ParserMode::FullParse
    }
    fn grammar_id() -> String {
        "host-utils-test".into()
    }
    fn detect_language(_: String, _: String) -> String {
        "host-utils-test".into()
    }
    fn process(input: String, _: String, _: String) -> String {
        let request: Value = serde_json::from_str(&input).expect("test request JSON");
        let tree = request["tree"].as_str().expect("tree JSON string");
        let trivia: Vec<String> = serde_json::from_value(request["trivia"].clone()).unwrap();
        match request["operation"].as_str().unwrap() {
            "strip" => host_utils::strip_trivia(tree, &trivia),
            "hash" => serde_json::to_string(&host_utils::structural_hash(tree)).unwrap(),
            "ignore-strip-error" => {
                let _ = host_utils::strip_trivia(tree, &trivia);
                "{}".into()
            }
            "ignore-hash-error" => {
                let _ = host_utils::structural_hash(tree);
                "{}".into()
            }
            _ => panic!("unknown test operation"),
        }
    }
    fn trivia_node_types() -> Vec<String> {
        vec![]
    }
    fn language_ids() -> Vec<String> {
        vec!["host-utils-test".into()]
    }
    fn language_info() -> Vec<LanguageInfoRecord> {
        vec![]
    }
    fn priority() -> i32 {
        0
    }
    fn preprocess_source(source: String) -> String {
        source
    }
    fn example(_: String) -> ExamplePair {
        ExamplePair {
            old: "{}".into(),
            new: "{}".into(),
        }
    }
}

export!(Probe);
