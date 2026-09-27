//! Bundled-parser manifest resolution using the shared candidate planner.
//! Special filenames and extensions use the same matching rules as bindings.
//! Unknown names retain the generic native route. Full capability probing and
//! failure/priority selection across discovered components remain tracked in #108.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Deserialize;

#[derive(Deserialize)]
struct ParserEntry {
    plugin_id: String,
    wasm: String,
    #[serde(default)]
    extensions: Vec<String>,
}

#[derive(Deserialize)]
struct Manifest {
    parsers: HashMap<String, ParserEntry>,     // language_id -> entry
    extension_index: HashMap<String, String>,  // ".ext" -> language_id
}

/// A resolved parser for a file. `wasm_path` is "" for the native-Python path (parsed in-core via
/// tree-sitter-python — no wasm needed).
pub(crate) struct ResolvedParser {
    pub language: String,
    pub plugin_id: String,
    pub wasm_path: String,
}

fn manifest_cache() -> &'static Mutex<HashMap<String, Option<Arc<Manifest>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Manifest>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load + cache `<wasm_dir>/parser_manifest.json` (per wasm_dir; the dir is stable per process).
fn load_manifest(wasm_dir: &str) -> Option<Arc<Manifest>> {
    let cache = manifest_cache();
    if let Ok(map) = cache.lock() {
        if let Some(entry) = map.get(wasm_dir) {
            return entry.clone();
        }
    }
    let parsed = std::fs::read_to_string(Path::new(wasm_dir).join("parser_manifest.json"))
        .ok()
        .and_then(|data| serde_json::from_str::<Manifest>(&data).ok())
        .map(Arc::new);
    if let Ok(mut map) = cache.lock() {
        map.insert(wasm_dir.to_owned(), parsed.clone());
    }
    parsed
}

/// Resolve the parser for *path* by special filename or extension from the manifest.
/// Returns `None` for an unknown extension or a missing/invalid manifest → the caller falls back.
pub(crate) fn resolve_parser(path: &str, wasm_dir: &str) -> Option<ResolvedParser> {
    let manifest = load_manifest(wasm_dir)?;
    let ext = Path::new(path)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()));
    let mut languages: Vec<&String> = manifest.parsers.keys().collect();
    languages.sort();
    let candidates: Vec<crate::parser_routing::Candidate> = languages.iter().map(|language| {
        let entry = &manifest.parsers[*language];
        let mut extensions = entry.extensions.clone();
        extensions.extend(manifest.extension_index.iter().filter(|(_, lang)| *lang == *language).map(|(extension, _)| extension.clone()));
        crate::parser_routing::Candidate {
            id: (*language).clone(), aliases: vec![entry.plugin_id.clone()],
            languages: vec![(*language).clone()], extensions,
            // Preserve the manifest's declared winner for ambiguous extensions.
            priority: i32::from(ext.as_ref().and_then(|extension| manifest.extension_index.get(extension)) == Some(*language)),
            ..Default::default()
        }
    }).collect();
    let indices = crate::parser_routing::filename_candidates(&candidates, path).ok()?;
    let language = match indices.first() {
        Some(&index) => languages[index],
        None => manifest.parsers.get_key_value("generic")?.0,
    };
    let entry = manifest.parsers.get(language)?;
    let wasm_path = if language == "python" {
        String::new()
    } else {
        Path::new(wasm_dir)
            .join(&entry.wasm)
            .to_string_lossy()
            .into_owned()
    };
    Some(ResolvedParser {
        language: language.clone(),
        plugin_id: entry.plugin_id.clone(),
        wasm_path,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_manifest_recognizes_special_filenames() {
        let directory = std::env::temp_dir().join(format!("intentumdiff-routing-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("parser_manifest.json"), r#"{"parsers":{"generic":{"plugin_id":"generic","wasm":"generic.wasm","extensions":[".txt"]},"cmake":{"plugin_id":"cmake","wasm":"cmake.wasm","extensions":["CMakeLists.txt"]},"dockerfile":{"plugin_id":"dockerfile","wasm":"docker.wasm","extensions":["Dockerfile"]}},"extension_index":{".txt":"generic"}}"#).unwrap();
        for (filename, language) in [("CMakeLists.txt","cmake"),("Dockerfile","dockerfile"),("unknown","generic")] {
            assert_eq!(super::resolve_parser(filename,directory.to_str().unwrap()).unwrap().language,language);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
