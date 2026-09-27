use intentumdiff_rust_core::filename_selection::{next_action, Action, Request, Event};
use serde_json::{json, Value};

#[test]
fn source_expected_selection_corpus() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/filename_selection.json")).unwrap();
    for case in cases {
        let request: Request = serde_json::from_value(case["request"].clone()).unwrap();
        let events: Vec<Event> = serde_json::from_value(case["events"].clone()).unwrap();
        let actual = next_action(&request, &events);
        if let Some(error) = case["error"].as_str() {
            assert!(actual.unwrap_err().contains(error), "{}", case["name"]);
        } else {
            assert_eq!(serde_json::to_value(actual.unwrap()).unwrap(), case["expected"], "{}", case["name"]);
            // Every observation must have been requested by the preceding prefix.
            for (end, event) in events.iter().enumerate() {
                let action = next_action(&request, &events[..end]).unwrap();
                match (action, event) {
                    (Action::Load { index: a }, Event::Loaded { index: b, .. } | Event::LoadFailed { index: b, .. }) => assert_eq!(a,*b),
                    (Action::Probe { index: a, .. }, Event::Probed { index: b, .. } | Event::ProbeFailed { index: b }) => assert_eq!(a,*b),
                    _ => panic!("unexpected replay action in {}",case["name"]),
                }
            }
        }
    }
}

#[test]
fn sample_is_utf8_safe() {
    for character in ["é", "😀"] {
        let request: Request = serde_json::from_value(json!({"entries":[{"id":"py","languages":["python"],"extensions":[".py"]}],"filename":"a.py","content":format!("{}{character}","a".repeat(2047))})).unwrap();
        let event = Event::Loaded { index: 0, grammar_id:"python".into(), languages:vec!["python".into()], priority:0 };
        assert_eq!(next_action(&request,&[event]).unwrap(),Action::Probe{index:0,sample:"a".repeat(2047)});
    }
}
