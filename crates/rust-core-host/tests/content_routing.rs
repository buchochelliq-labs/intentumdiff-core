use intentumdiff_rust_core::content_type::{detect_content_type, HEAD_BYTES};
#[test]
fn source_judged_content_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/content_routing.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let bytes: Vec<u8> = serde_json::from_value(case["bytes"].clone()).unwrap();
        assert_eq!(detect_content_type(&bytes).is_text, case["is_text"].as_bool().unwrap(), "{}", case["name"]);
    }
}
#[test]
fn sampling_window_belongs_to_engine() {
    let mut bytes = vec![b'a'; HEAD_BYTES]; bytes.push(0);
    assert!(detect_content_type(&bytes).is_text);
    bytes[HEAD_BYTES - 1] = 0;
    assert!(!detect_content_type(&bytes).is_text);
}
