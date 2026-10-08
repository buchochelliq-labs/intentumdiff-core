use std::path::PathBuf;
use std::process::Command;

#[test]
fn file_diff_preserves_both_input_paths_in_json_and_rich_output() {
    let staged = PathBuf::from(
        std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
            .expect("set INTENTUMDIFF_TEST_WASM_DIR to verified parser components"),
    );
    assert!(
        staged.join("python_parser.wasm").is_file(),
        "verified Python parser missing"
    );
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "intentumdiff-cli-paths-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("before")).unwrap();
    std::fs::create_dir_all(root.join("after")).unwrap();
    // CI provisions checksum-verified raw components, not a packaged inventory.
    // Give this fixture an explicit inventory without weakening component checks.
    let parsers = root.join("parsers");
    std::fs::create_dir_all(&parsers).unwrap();
    std::fs::copy(
        staged.join("python_parser.wasm"),
        parsers.join("python_parser.wasm"),
    )
    .unwrap();
    std::fs::write(parsers.join("parser_manifest.json"), serde_json::json!({
        "parsers": {"python": {"plugin_id": "python", "wasm": "python_parser.wasm", "extensions": [".py"]}},
        "extension_index": {".py": "python"}
    }).to_string()).unwrap();
    let wasm = parsers.to_string_lossy().into_owned();
    for (old, new) in [
        ("def f():\n    return 1\n", "def f():\n    return 1\n"),
        ("def f():\n    return 1\n", "def f():\n    return 2\n"),
        ("def f(", "def g("),
    ] {
        std::fs::write(root.join("before/sample.py"), old).unwrap();
        std::fs::write(root.join("after/sample.py"), new).unwrap();
        for as_json in [true, false] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_intentumdiff"));
            command.current_dir(&root).env("NO_COLOR", "1").args([
                "file",
                "before/sample.py",
                "after/sample.py",
                "--wasm-dir",
                &wasm,
            ]);
            if as_json {
                command.arg("--json");
            }
            let output = command.output().expect("spawn actual native CLI");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let rendered = String::from_utf8(output.stdout).unwrap();
            if as_json {
                let diff: serde_json::Value = serde_json::from_str(&rendered).unwrap();
                assert_eq!(diff["old_filename"], "before/sample.py", "{rendered}");
                assert_eq!(diff["new_filename"], "after/sample.py", "{rendered}");
            } else {
                assert!(rendered.contains("Old: before/sample.py"), "{rendered}");
                assert!(rendered.contains("New: after/sample.py"), "{rendered}");
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
