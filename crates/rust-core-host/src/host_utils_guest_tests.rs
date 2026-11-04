//! Real component imports, checked against independently specified examples.
use super::*;

fn guest_request(operation: &str, tree: &str, trivia: &[String]) -> String {
    json!({"operation": operation, "tree": tree, "trivia": trivia}).to_string()
}

fn through_guest(old: &str, new: &str) -> Result<WasmProcessPair, String> {
    let wasm = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/host-utils-guest/target/wasm32-wasip2/debug/host_utils_guest.wasm");
    assert!(
        wasm.is_file(),
        "build the test-only host-utils guest before running this gate"
    );
    run_python_wasm_process_pair_detailed(
        wasm.to_str().unwrap(),
        old,
        "",
        "old.json",
        new,
        "",
        "new.json",
        2_000_000_000,
        16 * 1024 * 1024,
        "host-utils-test",
    )
}

#[test]
fn fullparse_guest_imports_match_documented_tree_examples() {
    let cases: Value =
        serde_json::from_str(include_str!("../tests/fixtures/host_utils.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let operation = case["operation"].as_str().unwrap();
        let tree = case["tree"].as_str().unwrap();
        let trivia: Vec<String> = serde_json::from_value(case["trivia"].clone()).unwrap();
        let direct = if operation == "strip" {
            host_utils::strip_trivia(tree, &trivia, &host_utils::Limits::default())
        } else {
            host_utils::structural_hash(tree, &host_utils::Limits::default()).map(Value::String)
        };
        let request = guest_request(operation, tree, &trivia);
        let guest = through_guest(&request, &request);
        if let Some(error) = case["error_contains"].as_str() {
            assert!(direct.unwrap_err().contains(error), "{}", case["id"]);
            assert!(
                guest.err().expect("guest must fail").contains(error),
                "{}",
                case["id"]
            );
        } else {
            assert_eq!(direct.unwrap(), case["expected"], "{}", case["id"]);
            let pair = guest.unwrap();
            for output in [pair.old_tree, pair.new_tree] {
                assert_eq!(
                    serde_json::from_str::<Value>(&output).unwrap(),
                    case["expected"],
                    "{}",
                    case["id"]
                );
            }
        }
    }
}

#[test]
fn fullparse_guest_cannot_hide_host_errors_in_either_source() {
    let valid = guest_request("strip", r#"{"type":"root"}"#, &[]);
    // The malicious test guest ignores the host result and returns a success-shaped object.
    for operation in ["ignore-strip-error", "ignore-hash-error"] {
        let invalid = guest_request(operation, "not JSON", &[]);
        for (old, new) in [(&invalid, &valid), (&valid, &invalid)] {
            assert!(through_guest(old, new)
                .err()
                .expect("guest must fail")
                .contains("invalid JSON"));
        }
    }
}

#[test]
fn fullparse_guest_imports_enforce_default_limits() {
    let limits = host_utils::Limits::default();
    let deep = format!(
        "{}0{}",
        "[".repeat(limits.max_depth + 1),
        "]".repeat(limits.max_depth + 1)
    );
    let oversized = " ".repeat(limits.max_bytes + 1);
    let many_nodes = format!(
        "{{\"children\":[{}]}}",
        vec!["{}"; limits.max_nodes].join(",")
    );
    let cases = [
        (deep, vec![], "nesting depth"),
        (many_nodes, vec![], "node count"),
        (oversized, vec![], "byte limit"),
        (
            "{}".into(),
            vec!["comment".into(); limits.max_trivia_types + 1],
            "type count",
        ),
        (
            "{}".into(),
            vec!["x".repeat(limits.max_trivia_type_bytes + 1)],
            "type exceeds",
        ),
        (
            "{}".into(),
            vec!["x".repeat(limits.max_trivia_type_bytes); 257],
            "payload exceeds",
        ),
    ];
    for (tree, trivia, error) in cases {
        let direct = host_utils::strip_trivia(&tree, &trivia, &limits).unwrap_err();
        assert!(direct.contains(error), "{direct}");
        let request = guest_request("strip", &tree, &trivia);
        let actual = through_guest(&request, &request)
            .err()
            .expect("guest must fail");
        assert!(actual.contains(error), "{actual}");
        if trivia.is_empty() {
            assert!(host_utils::structural_hash(&tree, &limits)
                .unwrap_err()
                .contains(error));
            let request = guest_request("hash", &tree, &trivia);
            let actual = through_guest(&request, &request)
                .err()
                .expect("hash guest must fail");
            assert!(actual.contains(error), "{actual}");
        }
    }
}
