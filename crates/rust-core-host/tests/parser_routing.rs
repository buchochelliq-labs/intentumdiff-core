use intentumdiff_rust_core::parser_routing::{Candidate, RoutingQuery, shortlist};

#[test]
fn generic_is_last_even_when_no_filename_matches() {
    let candidates = vec![
        Candidate { id: "generic".into(), languages: vec!["generic".into()], ..Default::default() },
        Candidate { id: "specific".into(), languages: vec!["python".into()], ..Default::default() },
    ];
    assert_eq!(shortlist(&candidates, &RoutingQuery { filename: "unknown".into(), ..Default::default() }).unwrap(), vec![1,0]);
}

#[test]
fn source_judged_routing_corpus() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("fixtures/parser_routing.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let entries: Vec<Candidate> = serde_json::from_value(case["entries"].clone()).unwrap();
        let query: RoutingQuery = serde_json::from_value(case["query"].clone()).unwrap();
        let ids: Vec<&str> = shortlist(&entries, &query).unwrap().into_iter().map(|i|entries[i].id.as_str()).collect();
        assert_eq!(serde_json::to_value(ids).unwrap(),case["expected"],"{}",case["name"]);
    }
}
