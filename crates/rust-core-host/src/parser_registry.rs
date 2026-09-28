//! Bundled component host for the shared filename selection policy.
//! The manifest supplies discovery hints; only loaded guest metadata and probes
//! decide capability. No language-specific fallback policy lives in this host.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Deserialize;
use serde_json::Value;
use crate::filename_selection::{Action, Event, Request};
use crate::parser_routing::Candidate;
use wasmtime::{component::{HasSelf, Linker}, Store};

#[derive(Deserialize)]
struct ParserEntry {
    plugin_id: String,
    wasm: String,
    #[serde(default)]
    extensions: Vec<String>,
}

#[derive(Deserialize)]
struct Manifest {
    parsers: HashMap<String, ParserEntry>,
    extension_index: HashMap<String, String>,
}

pub(crate) struct ResolvedParser {
    pub language: String,
    pub plugin_id: String,
    pub wasm_path: String,
}

fn manifest_cache() -> &'static Mutex<HashMap<String, Option<Arc<Manifest>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Manifest>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_manifest(wasm_dir: &str) -> Result<Option<Arc<Manifest>>, String> {
    let cache = manifest_cache();
    if let Ok(map) = cache.lock() {
        if let Some(entry) = map.get(wasm_dir) { return Ok(entry.clone()); }
    }
    let path = Path::new(wasm_dir).join("parser_manifest.json");
    let data = match std::fs::read_to_string(&path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read parser manifest {}: {e}", path.display())),
    };
    let parsed = Some(Arc::new(serde_json::from_str::<Manifest>(&data)
        .map_err(|e| format!("invalid parser manifest {}: {e}", path.display()))?));
    if let Ok(mut map) = cache.lock() { map.insert(wasm_dir.to_owned(), parsed.clone()); }
    Ok(parsed)
}

// A component can advertise several language aliases in the manifest. Load and
// probe it once, retaining every alias for explicit selection and discovery.
fn catalog(manifest: &Manifest) -> Vec<(Candidate, String)> {
    let mut grouped: BTreeMap<String, Candidate> = BTreeMap::new();
    let mut languages: Vec<_> = manifest.parsers.keys().collect();
    languages.sort();
    for language in languages {
        let entry = &manifest.parsers[language];
        let candidate = grouped.entry(entry.wasm.clone()).or_insert_with(|| Candidate {
            id: entry.plugin_id.clone(), ..Default::default()
        });
        candidate.languages.push(language.clone());
        candidate.aliases.extend([entry.plugin_id.clone(), language.clone()]);
        candidate.extensions.extend(entry.extensions.clone());
        candidate.extensions.extend(manifest.extension_index.iter()
            .filter(|(_, lang)| *lang == language).map(|(ext, _)| ext.clone()));
    }
    grouped.into_iter().map(|(path, mut candidate)| {
        candidate.aliases.sort(); candidate.aliases.dedup();
        candidate.extensions.sort(); candidate.extensions.dedup();
        (candidate, path)
    }).collect()
}

#[derive(Debug)]
struct HostError { message: String, terminal: bool }
impl From<wasmtime::Error> for HostError {
    fn from(error: wasmtime::Error) -> Self {
        // Runtime traps are terminal: fuel/sandbox failures must not silently
        // switch parser or get converted to a successful fallback result.
        Self { terminal: error.downcast_ref::<wasmtime::Trap>().is_some(), message: format!("{error:#}") }
    }
}

struct ComponentProbe {
    store: Store<crate::ParserHostState>,
    bindings: crate::parser_plugin::ParserPlugin,
    fuel: u64,
}
impl ComponentProbe {
    fn load(path: &str, fuel: u64) -> Result<Self, HostError> {
        let cached = crate::cached_parser_component(path, fuel != u64::MAX)
            .map_err(|message| HostError { message, terminal: false })?.cached;
        let mut linker = Linker::new(&cached.engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        crate::parser_plugin::ParserPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
        let mut store = Store::new(&cached.engine, crate::ParserHostState::new());
        if fuel != u64::MAX { store.set_fuel(fuel)?; }
        let bindings = crate::parser_plugin::ParserPlugin::instantiate(&mut store, &cached.component, &linker)?;
        Ok(Self { store, bindings, fuel })
    }
    fn reset(&mut self) -> Result<(), HostError> {
        if self.fuel != u64::MAX { self.store.set_fuel(self.fuel)?; }
        Ok(())
    }
    fn check_host(&mut self) -> Result<(), HostError> {
        match self.store.data_mut().host_error.take() {
            Some(message) => Err(HostError { message, terminal: true }), None => Ok(())
        }
    }
    fn metadata(&mut self, index: usize) -> Result<Event, HostError> {
        self.reset()?;
        let grammar_id = self.bindings.intentdiff_plugin_parser().call_grammar_id(&mut self.store)?;
        self.check_host()?;
        self.reset()?;
        let languages = self.bindings.intentdiff_plugin_parser().call_language_ids(&mut self.store)?;
        self.check_host()?;
        self.reset()?;
        let priority = self.bindings.intentdiff_plugin_parser().call_priority(&mut self.store)?;
        self.check_host()?;
        Ok(Event::Loaded { index, grammar_id, languages, priority })
    }
    fn detect(&mut self, filename: &str, sample: &str) -> Result<String, HostError> {
        self.reset()?;
        let language = self.bindings.intentdiff_plugin_parser().call_detect_language(&mut self.store, filename, sample)?;
        self.check_host()?;
        Ok(language)
    }
}

/// Resolve using the same action protocol as Python. Terminal component failures
/// propagate as errors; only a genuinely exhausted inventory returns `None`.
pub(crate) fn resolve_parser(path: &str, content: &str, config: &Value, wasm_dir: &str)
    -> Result<Option<ResolvedParser>, String>
{
    let Some(manifest) = load_manifest(wasm_dir)? else { return Ok(None); };
    let catalog = catalog(&manifest);
    let request = Request {
        entries: catalog.iter().map(|(candidate, _)| candidate.clone()).collect(),
        filename: path.into(), content: content.into(),
        language_hint: config.get("language_hint").and_then(Value::as_str).map(str::to_owned),
        plugin_id: config.get("plugin_id").and_then(Value::as_str).map(str::to_owned),
        strict: config.get("strict_plugins").and_then(Value::as_bool).unwrap_or(false),
        allowed_plugins: config.get("allowed_plugins").filter(|v| !v.is_null())
            .map(|v| serde_json::from_value(v.clone())).transpose()
            .map_err(|e| format!("invalid allowed_plugins: {e}"))?,
    };
    let fuel = crate::RustCoreConfig::from_json(&config.to_string()).plugin_fuel;
    let mut events = Vec::new();
    let mut loaded = HashMap::new();
    let mut errors = HashMap::new();
    loop {
        match crate::filename_selection::next_action(&request, &events)? {
            Action::Load { index } => {
                let component_path = Path::new(wasm_dir).join(&catalog[index].1);
                let outcome = ComponentProbe::load(&component_path.to_string_lossy(), fuel)
                    .and_then(|mut probe| { let event = probe.metadata(index)?; Ok((probe, event)) });
                match outcome {
                    Ok((probe, event)) => { loaded.insert(index, probe); events.push(event); }
                    Err(error) => {
                        errors.insert(events.len(), format!("parser {}: {}", catalog[index].0.id, error.message));
                        events.push(Event::LoadFailed { index, terminal: error.terminal });
                    }
                }
            }
            Action::Probe { index, sample } => {
                match loaded.get_mut(&index).expect("policy probes loaded components").detect(path, &sample) {
                    Ok(language) => events.push(Event::Probed { index, language: Some(language) }),
                    Err(error) => {
                        errors.insert(events.len(), format!("parser {}: {}", catalog[index].0.id, error.message));
                        events.push(Event::ProbeFailed { index });
                    }
                }
            }
            Action::Selected { index, language } => {
                let plugin_id = catalog[index].0.id.clone();
                // Only the certified first-party Python route can use its native
                // execution path, after real metadata loading and selection.
                let wasm_path = if language == "python" && crate::is_supported_python_plugin_id(&plugin_id) {
                    String::new()
                } else { Path::new(wasm_dir).join(&catalog[index].1).to_string_lossy().into_owned() };
                return Ok(Some(ResolvedParser { language, plugin_id, wasm_path }));
            }
            Action::NotFound { .. } => return Ok(None),
            Action::Failure { event } => return Err(errors.remove(&event).expect("recorded host error")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_groups_component_aliases_without_inventing_runtime_priority() {
        let manifest: Manifest = serde_json::from_value(serde_json::json!({
            "parsers": {"c":{"plugin_id":"c","wasm":"c.wasm","extensions":[".c"]},
                "cpp":{"plugin_id":"cpp","wasm":"c.wasm","extensions":[".cpp"]},
                "dockerfile":{"plugin_id":"dockerfile","wasm":"docker.wasm","extensions":["Dockerfile"]}},
            "extension_index": {".h":"cpp"}
        })).unwrap();
        let entries = catalog(&manifest);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.languages, ["c", "cpp"]);
        assert!(entries[0].0.aliases.contains(&"cpp".into()));
        assert!(entries[0].0.extensions.contains(&".h".into()));
        assert_eq!(entries[0].0.priority, 0);
        let candidates: Vec<_> = entries.into_iter().map(|(c,_)|c).collect();
        assert_eq!(crate::parser_routing::filename_candidates(&candidates,"Dockerfile").unwrap(), [1]);
    }
    #[test]
    fn fuel_traps_are_terminal_but_missing_components_are_not() {
        assert!(HostError::from(wasmtime::Error::new(wasmtime::Trap::OutOfFuel)).terminal);
        assert!(!ComponentProbe::load("/missing/intentumdiff-parser.wasm", 10_000).err().unwrap().terminal);
    }
    #[test]
    fn malformed_manifest_is_an_error_and_missing_manifest_can_be_retried() {
        let directory = tempfile::tempdir().unwrap();
        let dir = directory.path().to_str().unwrap();
        assert!(load_manifest(dir).unwrap().is_none());
        std::fs::write(directory.path().join("parser_manifest.json"), "{").unwrap();
        assert!(load_manifest(dir).err().unwrap().contains("invalid parser manifest"));
        std::fs::write(directory.path().join("parser_manifest.json"), r#"{"parsers":{},"extension_index":{}}"#).unwrap();
        assert!(load_manifest(dir).unwrap().is_some());
    }

    #[cfg(feature = "tier-c-wasm")]
    fn python_inventory(plugin_id: &str) -> tempfile::TempDir {
        let source = std::path::PathBuf::from(std::env::var("INTENTUMDIFF_TEST_WASM_DIR")
            .expect("set INTENTUMDIFF_TEST_WASM_DIR to provisioned components")).join("python_parser.wasm");
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(source, dir.path().join("python.wasm")).unwrap();
        std::fs::write(dir.path().join("parser_manifest.json"), serde_json::json!({
            "parsers":{"python":{"plugin_id":plugin_id,"wasm":"python.wasm","extensions":[".py"]}},
            "extension_index":{".py":"python"}
        }).to_string()).unwrap();
        dir
    }

    #[cfg(feature = "tier-c-wasm")]
    #[test]
    fn real_component_selection_preserves_custom_python_and_fuel_errors() {
        let custom = python_inventory("custom-python");
        let dir = custom.path().to_str().unwrap();
        let selected = resolve_parser("example.py", "", &serde_json::json!({}), dir).unwrap().unwrap();
        assert_eq!(selected.language, "python");
        assert!(!selected.wasm_path.is_empty(), "custom Python parser must execute its component");
        assert!(resolve_parser("example.py", "", &serde_json::json!({"allowed_plugins":[]}), dir).unwrap().is_none());
        let fuel_error = resolve_parser("example.py", "", &serde_json::json!({"plugin_fuel":0}), dir).err().unwrap();
        assert!(fuel_error.contains("fuel"), "{fuel_error}");
        let error = crate::live_server::live_diff_contents_impl(dir, "example.py", "x=1", "x=2",
            r#"{"plugin_fuel":0}"#, dir).unwrap_err();
        assert!(error.contains("fuel"), "must propagate rather than return fallback: {error}");
        assert!(crate::parse_to_tree("example.py", "x=2", r#"{"plugin_fuel":0}"#, dir).unwrap_err().contains("fuel"));

        let builtin = python_inventory("python");
        let selected = resolve_parser("example.py", "", &serde_json::json!({}), builtin.path().to_str().unwrap()).unwrap().unwrap();
        assert!(selected.wasm_path.is_empty(), "retain certified execution after actual component selection");
    }

    #[cfg(feature = "tier-c-wasm")]
    #[test]
    fn commit_selection_fuel_failure_is_terminal() {
        let inventory = python_inventory("python");
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git").arg("-C").arg(repo.path()).args(args)
                .output().expect("git is required for commit integration tests");
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        };
        git(&["init"]);
        std::fs::write(repo.path().join("example.py"), "value = 1\n").unwrap();
        git(&["add", "example.py"]);
        git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "base"]);
        std::fs::write(repo.path().join("example.py"), "value = 2\n").unwrap();
        let error = crate::live_server::live_handle_review_impl(repo.path().to_str().unwrap(), "HEAD", "",
            r#"{"plugin_fuel":0}"#, inventory.path().to_str().unwrap()).unwrap_err();
        assert!(error.contains("fuel"), "{error}");
    }

    #[cfg(feature = "tier-c-wasm")]
    #[test]
    fn native_selected_python_source_corpus() {
        let inventory = python_inventory("python");
        let dir = inventory.path().to_str().unwrap();
        let cases: Vec<Value> = serde_json::from_str(include_str!("../tests/fixtures/native_parser_selection.json")).unwrap();
        for case in cases {
            let output = crate::live_server::live_diff_contents_impl(dir, case["filename"].as_str().unwrap(),
                case["old"].as_str().unwrap(), case["new"].as_str().unwrap(), "{}", dir).unwrap();
            let result: Value = serde_json::from_str(&output).unwrap();
            let diff = &result["diff"];
            assert_eq!(diff["language"], "python", "{result}");
            assert_eq!(diff["has_semantic_changes"], true);
            assert_eq!(diff["is_style_only"], false);
            let changes = diff["changes"].as_array().unwrap();
            if case["new"].as_str().unwrap().is_empty() || case["old"].as_str().unwrap().is_empty() {
                assert!(!changes.is_empty());
                let expected = if case["old"].as_str().unwrap().is_empty() { "ADDITION" } else { "DELETION" };
                assert!(changes.iter().all(|c| c["change_type"] == expected));
            } else {
                assert_eq!(changes.len(), 1);
                assert_eq!(changes[0]["change_type"], "MODIFICATION");
                assert_eq!(changes[0]["old_node"]["label"], "1");
                assert_eq!(changes[0]["new_node"]["label"], "2");
                assert_eq!(diff["change_groups"].as_array().unwrap().len(), 1);
            }
        }
    }

}
