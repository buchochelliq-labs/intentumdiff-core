// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

use super::edit_matrix_port::*;

// ==============================================================================================
// Rust LANGUAGE-PARITY harness, part 2 of 3: corpus location, the language -> dispatch table, and
// the engine calls the port needs (`tree_roots`, the mutation diff).
//
// The harness MUST live in the inline `#[cfg(test)]` tree: `native_wasm_single_diff` is
// `pub(crate)` and the batch wrappers are crate-private, and a child module can see its
// ancestors' private items. A `tests/` integration test cannot reach either.
// ==============================================================================================

// ----------------------------------------------------------------------------------------------
// Locating the corpus and the parser components.
// ----------------------------------------------------------------------------------------------

fn env_dir(names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .find_map(|name| std::env::var_os(name))
        .map(PathBuf::from)
}

/// Repo-root corpus (`tests/fixtures/corpus`). `INTENTUMDIFF_TEST_CORPUS_DIR` (or the pre-rebrand
/// `INTENTDIFF_TEST_CORPUS_DIR`) overrides it so an extracted engine repo (#82) can point at a
/// vendored copy. Sentinel walk, not a hard-coded `../../tests`: depth-hardcoding breaks at the
/// split, and this crate is already built from nested worktrees.
pub(super) fn corpus_dir() -> PathBuf {
    if let Some(dir) = env_dir(&["INTENTUMDIFF_TEST_CORPUS_DIR", "INTENTDIFF_TEST_CORPUS_DIR"]) {
        return dir;
    }
    for ancestor in Path::new(env!("CARGO_MANIFEST_DIR")).ancestors() {
        let candidate = ancestor.join("tests").join("fixtures").join("corpus");
        if candidate.join("GENERATION_REPORT.json").is_file() {
            return candidate;
        }
    }
    panic!(
        "edit-matrix corpus not found: walked ancestors of {} looking for \
         tests/fixtures/corpus/GENERATION_REPORT.json. Set INTENTUMDIFF_TEST_CORPUS_DIR.",
        env!("CARGO_MANIFEST_DIR")
    );
}

/// Staged parser components. `src/intentdiff/wasm/*.wasm` is gitignored, so a fresh clone has
/// none - see `wasm_is_staged`.
pub(super) fn staged_wasm_dir() -> PathBuf {
    if let Some(dir) = env_dir(&["INTENTUMDIFF_TEST_WASM_DIR", "INTENTDIFF_TEST_WASM_DIR"]) {
        return dir;
    }
    for ancestor in Path::new(env!("CARGO_MANIFEST_DIR")).ancestors() {
        for package in ["intentumdiff", "intentdiff"] {
            let candidate = ancestor.join("src").join(package).join("wasm");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/intentdiff/wasm")
}

/// True when at least one parser component is staged. This is the harness's TIER-C gate: with no
/// components only the wasm-free dispatches (python's certified native batch, generic's native
/// text review) can run, and the parity test says so out loud instead of shrinking silently.
pub(super) fn wasm_is_staged() -> bool {
    let dir = staged_wasm_dir();
    match fs::read_dir(&dir) {
        Ok(entries) => entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with("_parser.wasm")
        }),
        Err(_) => false,
    }
}

pub(super) fn component_path(name: &str) -> PathBuf {
    staged_wasm_dir().join(name)
}

// ----------------------------------------------------------------------------------------------
// The dispatch table.
// ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Dispatch {
    /// The CERTIFIED native batch (`python_parser_backend: "native"`) - what `SemanticDiffer`
    /// actually runs for Python. Needs NO wasm.
    NativeBatch,
    /// `native_wasm_single_diff` short-circuits to `native_generic_text_diff` before touching a
    /// component.
    GenericText,
    /// Routed Wasm single diff through the named component.
    Wasm(&'static str),
    /// Same, with an identity-derived filename (`<language>_parser.wasm`).
    WasmOwned(String),
}

impl Dispatch {
    pub fn component(&self) -> Option<String> {
        match self {
            Dispatch::Wasm(name) => Some((*name).to_owned()),
            Dispatch::WasmOwned(name) => Some(name.clone()),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Dispatch::NativeBatch => "native-batch (python_parser_backend=native)".to_owned(),
            Dispatch::GenericText => "generic-text (native short-circuit)".to_owned(),
            other => format!("wasm  {}", other.component().unwrap_or_default()),
        }
    }
}

/// language -> parser component. This is PYTHON'S resolution (measured against
/// `PluginRegistry.detect_parser(manifest.filename, source, language_hint=manifest.language)`),
/// which is NOT `parser_manifest.json` - that file has no entry for 8 of the corpus languages
/// (`cmake, databricks, gomod, ini, make, proto, toml, wast`), a product gap the census test
/// pins. Where the manifest DOES have an entry, the census test cross-checks this table against
/// it so drift is caught.
///
/// MAINTAINER REVIEW POINT (deliberate, flagged): `python` takes the certified native batch, not
/// the (now full-parse) `python_parser.wasm`. Both run, but they build DIFFERENT trees - the batch
/// is `parse_python_tree -> serialize_ts_node -> strip_trivia -> convert_cst`, the component is
/// the Wasm grammar's own. `SemanticDiffer` demonstrably takes the batch, so the matrix must too;
/// routing it through the component would make the `python` row a test of a path no product
/// surface uses, and any divergence it found would be fabricated.
pub(super) fn component_for(language: &str) -> Dispatch {
    match language {
        "python" => Dispatch::NativeBatch,
        "generic" => Dispatch::GenericText,
        "c" | "cpp" => Dispatch::Wasm("cpp_parser.wasm"),
        "javascript" | "typescript" | "tsx" => Dispatch::Wasm("js_ts_parser.wasm"),
        "hcl" => Dispatch::Wasm("terraform_parser.wasm"),
        "m" => Dispatch::Wasm("dax_parser.wasm"),
        "wast" | "wat" => Dispatch::Wasm("wat_parser.wasm"),
        "databricks" | "databricks-workflow" => Dispatch::Wasm("databricks_parser.wasm"),
        other => Dispatch::WasmOwned(format!("{other}_parser.wasm")),
    }
}

/// The languages the FIRST INCREMENT covers. Chosen so one commit exercises every structural axis:
/// dispatch A (python), dispatch B (ini, asm), dispatch C (generic); an all-green language, a
/// language absent from `parser_manifest.json` whose manifest filename differs from the fixture
/// name, and a language carrying both a recorded `xfail` and a recorded `skip`.
pub(super) const INCREMENT_LANGUAGES: &[&str] = &["python", "generic", "ini", "asm"];

/// The languages the harness covers on this run. Defaults to `INCREMENT_LANGUAGES`; widening is
/// OPT-IN via `INTENTUMDIFF_EDIT_MATRIX_LANGUAGES` (a comma-separated list, or `all` for every
/// corpus language). This is how commit two extends the table: run `=all`, read the divergence
/// report, then promote the languages that are clean into `INCREMENT_LANGUAGES`. The env var can
/// only ADD coverage, never remove it from CI's default.
pub(super) fn selected_languages() -> Vec<String> {
    let selection = ["INTENTUMDIFF_EDIT_MATRIX_LANGUAGES", "INTENTDIFF_EDIT_MATRIX_LANGUAGES"]
        .iter()
        .find_map(|name| std::env::var(name).ok());
    match selection.as_deref() {
        None => INCREMENT_LANGUAGES.iter().map(|l| (*l).to_owned()).collect(),
        Some("all") => corpus_cases().into_iter().map(|case| case.language).collect(),
        Some(list) => list
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// Selected languages minus the ones whose component is not staged. A language is dropped ONLY
/// when nothing at all is staged (fresh clone); with a populated wasm dir a missing component is
/// a hard failure in `edit_matrix_every_component_is_staged`, never a silent shrink.
pub(super) fn runnable_increment_languages() -> Vec<String> {
    let staged = wasm_is_staged();
    selected_languages()
        .into_iter()
        .filter(|language| staged || component_for(language).component().is_none())
        .collect()
}

// ----------------------------------------------------------------------------------------------
// Corpus loading.
// ----------------------------------------------------------------------------------------------

/// `ok` is `true` in almost every cell and a STRING in at least one (`dart/duplicate_entity`).
/// Python only ever tests key PRESENCE, so `Option<Value>` models it exactly - an `Option<bool>`
/// would fail to deserialise the whole file, far from the cause.
#[derive(serde::Deserialize, Default, Clone, Debug)]
pub(super) struct Cell {
    #[serde(default)]
    pub ok: Option<Value>,
    #[serde(default)]
    pub skip: Option<String>,
    #[serde(default)]
    pub xfail: Option<String>,
}

impl Cell {
    pub fn recorded(&self) -> Recorded {
        if let Some(reason) = &self.skip {
            return Recorded::Skip(reason.clone());
        }
        if let Some(reason) = &self.xfail {
            return Recorded::Xfail(reason.clone());
        }
        Recorded::Ok
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Recorded {
    Ok,
    Skip(String),
    Xfail(String),
}

#[derive(serde::Deserialize, Debug, Clone)]
pub(super) struct PlaygroundManifest {
    pub language: String,
    pub filename: String,
    // visible_tokens / append_statement / mutation_xfail / invisible_tokens_at_generation are
    // unread here; serde ignores unknown fields by default.
}

#[derive(Clone, Debug)]
pub(super) struct CorpusCase {
    pub dir_name: String,
    pub language: String,
    /// The MANIFEST filename (e.g. `settings.ini`, `code.py`, `Makefile`) - never the fixture
    /// path. Language detection keys off it on both sides.
    pub filename: String,
    pub source: String,
    pub matrix: HashMap<String, Cell>,
}

/// Mirrors `_matrix_cases()`: dirs holding an `edit_matrix.expect.json`, sorted; a missing
/// `playground.expect.json` or a `playground.*` count != 1 DROPS the dir silently, exactly as
/// Python does. The census test then asserts the surviving count, so a drop is loud one layer up
/// instead of quietly shrinking the matrix.
pub(super) fn corpus_cases() -> Vec<CorpusCase> {
    let root = corpus_dir();
    let mut dir_names: Vec<String> = fs::read_dir(&root)
        .unwrap_or_else(|exc| panic!("read corpus dir {}: {exc}", root.display()))
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| entry.path().join("edit_matrix.expect.json").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    // Python sorts full glob paths under ONE parent, so sorting the directory names gives the
    // identical order.
    dir_names.sort();

    let mut cases = Vec::new();
    for dir_name in dir_names {
        let dir = root.join(&dir_name);
        let manifest_path = dir.join("playground.expect.json");
        if !manifest_path.exists() {
            continue;
        }
        let mut sources: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|exc| panic!("read corpus dir {}: {exc}", dir.display()))
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                name.starts_with("playground.") && !name.ends_with(".expect.json")
            })
            .collect();
        sources.sort();
        if sources.len() != 1 {
            continue;
        }
        let matrix_text = fs::read_to_string(dir.join("edit_matrix.expect.json"))
            .unwrap_or_else(|exc| panic!("read {dir_name}/edit_matrix.expect.json: {exc}"));
        let matrix: HashMap<String, Cell> = serde_json::from_str(&matrix_text)
            .unwrap_or_else(|exc| panic!("parse {dir_name}/edit_matrix.expect.json: {exc}"));
        // DELIBERATE DEVIATION from Python: Python does `matrix.get(verb, {})`, so an extra or
        // typo'd key is a silent no-op. Every committed file carries exactly the five verbs, so
        // pinning the key set is free and catches a class of ratchet corruption Python cannot see.
        let mut keys: Vec<&str> = matrix.keys().map(String::as_str).collect();
        keys.sort();
        let mut expected: Vec<&str> = EDIT_VERBS.iter().map(|v| v.name()).collect();
        expected.sort();
        assert_eq!(
            keys, expected,
            "{dir_name}/edit_matrix.expect.json must carry exactly the five verbs"
        );
        // Same deviation, one level down: exactly one of ok / skip / xfail per cell. An empty `{}`
        // silently reads as "must pass" in Python, and a cell carrying two of them is ambiguous.
        // (`ok` is `true` in almost every cell and a STRING in dart/duplicate_entity - which is
        // why the field is `Option<Value>`; this is the assertion that keeps it load-bearing.)
        for (verb, cell) in &matrix {
            let populated = [
                cell.ok.is_some(),
                cell.skip.is_some(),
                cell.xfail.is_some(),
            ]
            .into_iter()
            .filter(|set| *set)
            .count();
            assert_eq!(
                populated, 1,
                "{dir_name}/edit_matrix.expect.json[{verb}] must carry exactly one of \
                 ok / skip / xfail, found {populated}"
            );
        }
        let manifest_text = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|exc| panic!("read {dir_name}/playground.expect.json: {exc}"));
        let manifest: PlaygroundManifest = serde_json::from_str(&manifest_text)
            .unwrap_or_else(|exc| panic!("parse {dir_name}/playground.expect.json: {exc}"));
        let source = fs::read_to_string(&sources[0])
            .unwrap_or_else(|exc| panic!("read {}: {exc}", sources[0].display()));
        cases.push(CorpusCase {
            dir_name,
            language: manifest.language,
            filename: manifest.filename,
            source,
            matrix,
        });
    }
    cases
}

pub(super) fn corpus_case(language: &str) -> CorpusCase {
    corpus_cases()
        .into_iter()
        .find(|case| case.language == language)
        .unwrap_or_else(|| panic!("no corpus case for language {language:?}"))
}

// ----------------------------------------------------------------------------------------------
// The engine calls.
// ----------------------------------------------------------------------------------------------

/// The A/B/C router. Both sides use the MANIFEST filename.
pub(super) fn diff_one(
    language: &str,
    filename: &str,
    old: &str,
    new: &str,
) -> Result<Value, String> {
    match component_for(language) {
        Dispatch::NativeBatch => {
            let request = json!({
                "schema_version": 1,
                "config": {},
                // The Rust default is "wasm"; `rust_core.py` passes "native", which is what makes
                // this the certified batch (`certification=python_native_v4kb`).
                "python_parser_backend": PYTHON_PARSER_BACKEND_NATIVE,
                "files": [{
                    "old_source": old,
                    "new_source": new,
                    "old_filename": filename,
                    "new_filename": filename,
                    "language": "python",
                    "parser_wasm_path": ""
                }]
            });
            let raw = diff_batch(&request.to_string())?;
            let payload: Value = serde_json::from_str(&raw).map_err(|exc| exc.to_string())?;
            if payload["status"] == json!(FALLBACK) {
                return Err(format!(
                    "batch fell back: {}",
                    payload["reason"].as_str().unwrap_or("<no reason>")
                ));
            }
            let item = payload["diffs"]
                .get(0)
                .ok_or_else(|| "batch returned no diffs".to_owned())?;
            if item["status"] == json!(FALLBACK) {
                return Err(format!(
                    "batch item fell back: {}",
                    item["reason"].as_str().unwrap_or("<no reason>")
                ));
            }
            // The batch wraps in {"diff": ...} on some paths and returns the diff bare on others.
            // Normalise HERE - `MatrixDiff::from_value` panics rather than reading a null
            // `is_style_only`, but normalising once is what keeps that panic from ever firing.
            Ok(match item.get("diff") {
                Some(diff) if !diff.is_null() => diff.clone(),
                _ => item.clone(),
            })
        }
        dispatch => {
            let wasm = match dispatch.component() {
                None => String::new(), // generic: short-circuits before touching a component
                Some(name) => {
                    let path = component_path(&name);
                    if !path.is_file() {
                        return Err(format!(
                            "parser component {name} is not staged at {} - set \
                             INTENTUMDIFF_TEST_WASM_DIR, or stage src/intentdiff/wasm (gitignored)",
                            path.display()
                        ));
                    }
                    path.to_string_lossy().into_owned()
                }
            };
            crate::native_wasm_single_diff(
                language, &wasm, old, new, filename, filename, "{}", None,
            )
        }
    }
}

/// `_tree_roots`: NOT a parse call - it diffs "" against the source and keeps every change's
/// `new_node`.
pub(super) fn tree_roots(
    language: &str,
    filename: &str,
    source: &str,
) -> Result<Vec<MatrixNode>, String> {
    let payload = diff_one(language, filename, "", source)?;
    let diff = MatrixDiff::from_value(&format!("{language} tree_roots"), &payload);
    Ok(diff.changes.into_iter().filter_map(|c| c.new_node).collect())
}

// ----------------------------------------------------------------------------------------------
// Discovery cache.
// ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(super) struct Discovery {
    pub entities: Vec<Construct>,
    pub base: i64,
    pub literals: Vec<Construct>,
}

type DiscoveryCache = Mutex<HashMap<String, Result<Discovery, String>>>;

fn discovery_cache() -> &'static DiscoveryCache {
    static CACHE: OnceLock<DiscoveryCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Per verb Python runs three engine diffs (entity discovery, literal discovery, mutation).
/// Discovery is deterministic and pure in `(language, filename, source)`, so caching it - INCLUDING
/// a cached `Err`, which still skips every verb with the same message - is observationally
/// identical and cuts the diff count by ~40%.
pub(super) fn discover(
    language: &str,
    filename: &str,
    source: &str,
) -> Result<Discovery, String> {
    let key = format!("{language}\u{1f}{filename}\u{1f}{source}");
    if let Some(hit) = discovery_cache().lock().expect("discovery cache").get(&key) {
        return hit.clone();
    }
    let computed = (|| {
        let roots = tree_roots(language, filename, source)?;
        let (entities, base) = find_entities_from_roots(language, source, &roots);
        let literals = find_literals_from_roots(language, source, &roots);
        Ok(Discovery {
            entities,
            base,
            literals,
        })
    })();
    discovery_cache()
        .lock()
        .expect("discovery cache")
        .insert(key, computed.clone());
    computed
}

// ----------------------------------------------------------------------------------------------
// run_verb / build_manifest.
// ----------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Ok,
    Fail(String),
    Skip(String),
}

impl Status {
    pub fn kind(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Fail(_) => "fail",
            Status::Skip(_) => "skip",
        }
    }

    pub fn detail(&self) -> Option<&str> {
        match self {
            Status::Ok => None,
            Status::Fail(detail) | Status::Skip(detail) => Some(detail),
        }
    }
}

/// What a run produced, plus everything the divergence report needs.
#[derive(Clone, Debug)]
pub(super) struct VerbRun {
    pub status: Status,
    pub ctx: Option<Ctx>,
    pub mutated: Option<String>,
    pub diff: Option<MatrixDiff>,
    pub discovery: Option<Discovery>,
}

pub(super) fn run_verb(language: &str, filename: &str, source: &str, verb: Verb) -> VerbRun {
    // -- TOLERATED PHASE: discovery + derivation. Every engine call returns Result; nothing here
    //    may panic. Deliberately NO catch_unwind: Python's `except Exception` also swallows things
    //    Rust expresses as panics (index errors, AttributeError). If Rust panics where Python
    //    skipped, that is a genuine finding and must crash the test, not be laundered into a skip.
    let discovery = discover(language, filename, source);
    let discovery = match discovery {
        Err(exc) => {
            return VerbRun {
                status: Status::Skip(format!("raised: {exc}")),
                ctx: None,
                mutated: None,
                diff: None,
                discovery: None,
            }
        }
        Ok(discovery) => discovery,
    };
    // `find_literals` runs even for entity verbs (Python computes both unconditionally). Skipping
    // it lazily would change the skip surface: a parser gap in literal discovery currently turns a
    // `delete_entity` case into a skip.
    let pool: &[Construct] = match verb.needs() {
        "literals" => &discovery.literals,
        _ => &discovery.entities,
    };
    if pool.is_empty() {
        return VerbRun {
            status: Status::Skip(format!(
                "no {} discoverable in the semantic tree",
                verb.needs()
            )),
            ctx: None,
            mutated: None,
            diff: None,
            discovery: Some(discovery),
        };
    }
    let derived = derive(verb, source, pool, discovery.base);
    let (mutated, ctx) = match derived {
        None => {
            return VerbRun {
                status: Status::Skip(format!("{} not derivable from this snippet", verb.name())),
                ctx: None,
                mutated: None,
                diff: None,
                discovery: Some(discovery),
            }
        }
        Some(pair) => pair,
    };

    // -- NO TOLERANCE PAST THIS POINT: an engine error here is a product bug, never a skip.
    let payload = diff_one(language, filename, source, &mutated).unwrap_or_else(|exc| {
        panic!(
            "{language} [{}]: mutation diff failed: {exc}",
            verb.name()
        )
    });
    let diff = MatrixDiff::from_value(&format!("{language} [{}]", verb.name()), &payload);
    let problem = check(&diff, &ctx);
    VerbRun {
        status: match problem {
            None => Status::Ok,
            Some(problem) => Status::Fail(problem),
        },
        ctx: Some(ctx),
        mutated: Some(mutated),
        diff: Some(diff),
        discovery: Some(discovery),
    }
}

/// `build_manifest` - `ok -> {"ok": true}`, `skip -> {"skip": detail}`, `fail -> {"xfail": detail}`
/// in EDIT_VERBS insertion order. Rendered by hand because `serde_json::Map` is a BTreeMap here
/// (no `preserve_order` feature) and the committed files are in verb order, not alphabetical.
pub(super) fn build_manifest_json(language: &str, filename: &str, source: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("{".to_owned());
    for (index, verb) in EDIT_VERBS.iter().enumerate() {
        let run = run_verb(language, filename, source, *verb);
        let body = match &run.status {
            Status::Ok => "\"ok\": true".to_owned(),
            Status::Skip(detail) => format!("\"skip\": {}", json!(detail)),
            Status::Fail(detail) => format!("\"xfail\": {}", json!(detail)),
        };
        let comma = if index + 1 == EDIT_VERBS.len() { "" } else { "," };
        lines.push(format!(" {}: {{", json!(verb.name())));
        lines.push(format!("  {body}"));
        lines.push(format!(" }}{comma}"));
    }
    lines.push("}".to_owned());
    format!("{}\n", lines.join("\n"))
}

// ----------------------------------------------------------------------------------------------
// Tree dump - the first deliverable of the issue.
//
// Four wrong fix attempts were spent on an INI bug this week for want of a way to SEE the tree the
// engine built. `render_tree` prints exactly that: language, and per node its type, label,
// span and structural_hash.
// ----------------------------------------------------------------------------------------------

fn render_node(node: &MatrixNode, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    out.push_str(&format!(
        "{indent}{ty} {label} L{start}-{end} end_col={col} #{hash}\n",
        ty = node.node_type,
        label = py_repr(&node.label),
        start = node.position.start_line,
        end = node.position.end_line,
        col = node.position.end_col,
        hash = node.structural_hash,
    ));
    for child in &node.children {
        render_node(child, depth + 1, out);
    }
}

/// Render the semantic tree the engine builds for a corpus fixture.
pub(super) fn render_tree(language: &str, filename: &str, roots: &[MatrixNode]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "===== tree dump: language={language} filename={filename} dispatch={} roots={} =====\n",
        component_for(language).label(),
        roots.len()
    ));
    for (index, root) in roots.iter().enumerate() {
        out.push_str(&format!("--- root[{index}] ---\n"));
        render_node(root, 1, &mut out);
    }
    out
}

/// Render the tree for a corpus language by name. Returns the dump plus the discovered
/// entities/literals, which is what actually explains a wrong derivation.
pub(super) fn dump_fixture_tree(language: &str) -> Result<String, String> {
    let case = corpus_case(language);
    let roots = tree_roots(&case.language, &case.filename, &case.source)?;
    let mut out = render_tree(&case.language, &case.filename, &roots);
    let (entities, base) = find_entities_from_roots(&case.language, &case.source, &roots);
    let literals = find_literals_from_roots(&case.language, &case.source, &roots);
    out.push_str(&format!(
        "--- entities ({}, base={base}) ---\n",
        entities.len()
    ));
    for entity in &entities {
        out.push_str(&format!(
            "  {} {} L{}-{} end_col={} name={:?}\n",
            entity.node_type,
            py_repr(&entity.label),
            entity.start_line,
            entity.end_line,
            entity.end_col,
            entity_name(entity)
        ));
    }
    out.push_str(&format!("--- literals ({}) ---\n", literals.len()));
    for literal in &literals {
        out.push_str(&format!(
            "  {} {} L{}\n",
            literal.node_type,
            py_repr(&literal.label),
            literal.start_line
        ));
    }
    Ok(out)
}
