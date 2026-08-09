// Split from tests_inline.rs (issue #85): one file per test family.
// Nested inside cfg(test) mod tests - `super::*` = the tests mod (helpers),
// `crate::*` = the engine.
#![allow(unused_imports)]
use super::*;
use crate::*;

use super::edit_matrix_corpus::*;
use super::edit_matrix_port::*;

// `tests_inline.rs` also defines a `staged_wasm_dir`, so two glob imports offer the same name
// and every call site is ambiguous (E0659). An explicit import outranks a glob, which pins the
// harness to the corpus module's version - the one that honours INTENTUMDIFF_TEST_WASM_DIR.
use super::edit_matrix_corpus::staged_wasm_dir;

// ==============================================================================================
// Rust LANGUAGE-PARITY harness, part 3 of 3: the tests.
//
// INCREMENT ONE covers four languages (see `INCREMENT_LANGUAGES`), one per structural axis of the
// design. The remaining 69 corpus languages are a table extension, not new machinery.
//
// TIER-C GATE. This branch has no cargo features at all, so the wasm gate is the STAGED ARTIFACT
// (`src/intentdiff/wasm/*.wasm` is gitignored; a fresh clone has none). When nothing is staged the
// parity test runs the wasm-free dispatches only (python's certified native batch + generic's
// native text review) and SAYS SO - it never silently shrinks. When something IS staged, a missing
// component for a mapped language is a hard failure, never a skip.
// ==============================================================================================

// ----------------------------------------------------------------------------------------------
// Report rendering.
// ----------------------------------------------------------------------------------------------

struct Divergence {
    language: String,
    verb: Verb,
    recorded: Recorded,
    run: VerbRun,
    filename: String,
    headline: String,
}

fn render_divergence(index: usize, total: usize, divergence: &Divergence, source: &str) -> String {
    let mut out = String::new();
    let Divergence {
        language,
        verb,
        recorded,
        run,
        filename,
        headline,
    } = divergence;
    out.push_str(&format!(
        "\n[{}/{}] DIVERGENCE  {language} [{}]\n",
        index + 1,
        total,
        verb.name()
    ));
    out.push_str(&format!("  {headline}\n"));
    out.push_str(&format!(
        "  expected (tests/fixtures/corpus/{language}/edit_matrix.expect.json): {}\n",
        match recorded {
            Recorded::Ok => "ok".to_owned(),
            Recorded::Skip(reason) => format!("skip -- {reason}"),
            Recorded::Xfail(reason) => format!("xfail -- {reason}"),
        }
    ));
    out.push_str(&format!("  rust live:  {}\n", run.status.kind()));
    if let Some(detail) = run.status.detail() {
        out.push_str(&format!("  detail:     {detail}\n"));
    }
    out.push_str(&format!(
        "  dispatch:   {}   filename={filename}\n",
        component_for(language).label()
    ));
    if let Some(ctx) = &run.ctx {
        out.push_str(&format!("  ctx:        {}\n", ctx.render()));
    }
    if let Some(discovery) = &run.discovery {
        out.push_str(&format!(
            "  entities ({}):\n",
            discovery.entities.len()
        ));
        for entity in discovery.entities.iter().take(12) {
            out.push_str(&format!(
                "     {} {} L{}-{} end_col={}\n",
                entity.node_type,
                py_repr(&entity.label),
                entity.start_line,
                entity.end_line,
                entity.end_col
            ));
        }
        out.push_str(&format!("  literals ({}):", discovery.literals.len()));
        for literal in discovery.literals.iter().take(8) {
            out.push_str(&format!(
                " {} {} L{} |",
                literal.node_type,
                py_repr(&literal.label),
                literal.start_line
            ));
        }
        out.push('\n');
    }
    if let Some(diff) = &run.diff {
        out.push_str(&format!(
            "  diff: {} changes, is_style_only={}\n",
            diff.changes.len(),
            diff.is_style_only
        ));
        for change in diff.changes.iter().take(12) {
            let render = |node: &Option<MatrixNode>| match node {
                None => "None".to_owned(),
                Some(node) => format!("{}({})", node.node_type, py_repr(&node.label)),
            };
            out.push_str(&format!(
                "     {:<14} old={:<34} new={}\n",
                change.change_type,
                render(&change.old_node),
                render(&change.new_node)
            ));
        }
    }
    if let Some(mutated) = &run.mutated {
        out.push_str("  mutated source:\n");
        for (index, line) in py_splitlines(mutated).iter().enumerate().take(24) {
            out.push_str(&format!("   {:>3} | {line}\n", index + 1));
        }
    }
    out.push_str("  source:\n");
    for (index, line) in py_splitlines(source).iter().enumerate().take(24) {
        out.push_str(&format!("   {:>3} | {line}\n", index + 1));
    }
    out.push_str(&format!(
        "  reproduce:\n     cd crates/rust-core-host && cargo test edit_matrix -- --nocapture \
         --exact tests::edit_matrix::edit_matrix_parity_with_python_expectations\n"
    ));
    out
}

/// `rust live | python recorded` per verb.
fn render_census(
    live: &HashMap<(String, &'static str), u32>,
    recorded: &HashMap<(String, &'static str), u32>,
) -> String {
    let mut out = String::new();
    out.push_str(
        "\nCENSUS over EXECUTED cells only (rust live | python recorded). Recorded-skip cells are\n\
         short-circuited exactly as pytest.skip does and are audited by edit_matrix_skip_audit_*.\n",
    );
    out.push_str(&format!(
        "  {:<18}{:>5}{:>7}{:>7}   |{:>7}{:>7}{:>7}\n",
        "verb", "ok", "skip", "fail", "ok", "skip", "xfail"
    ));
    for verb in EDIT_VERBS {
        let get = |map: &HashMap<(String, &'static str), u32>, kind: &'static str| {
            *map.get(&(verb.name().to_owned(), kind)).unwrap_or(&0)
        };
        let (lo, ls, lf) = (get(live, "ok"), get(live, "skip"), get(live, "fail"));
        let (ro, rs, rx) = (
            get(recorded, "ok"),
            get(recorded, "skip"),
            get(recorded, "xfail"),
        );
        let flag = if lo == ro && ls == rs && lf == rx {
            ""
        } else {
            "   <-- differs"
        };
        out.push_str(&format!(
            "  {:<18}{lo:>5}{ls:>7}{lf:>7}   |{ro:>7}{rs:>7}{rx:>7}{flag}\n",
            verb.name()
        ));
    }
    out
}

fn dump_dir() -> Option<PathBuf> {
    ["INTENTUMDIFF_EDIT_MATRIX_DUMP", "INTENTDIFF_EDIT_MATRIX_DUMP"]
        .iter()
        .find_map(|name| std::env::var_os(name))
        .map(PathBuf::from)
}

/// `INTENTUMDIFF_EDIT_MATRIX_DUMP=<dir>` writes each language's LIVE manifest in the committed
/// shape, so a maintainer can run `git diff --no-index tests/fixtures/corpus <dir>` and read the
/// whole Rust-vs-Python delta as an ordinary diff.
fn write_dump(case: &CorpusCase) {
    let Some(root) = dump_dir() else { return };
    let dir = root.join(&case.dir_name);
    fs::create_dir_all(&dir).unwrap_or_else(|exc| panic!("create {}: {exc}", dir.display()));
    let rendered = build_manifest_json(&case.language, &case.filename, &case.source);
    let path = dir.join("edit_matrix.expect.json");
    fs::write(&path, rendered).unwrap_or_else(|exc| panic!("write {}: {exc}", path.display()));
}

// ----------------------------------------------------------------------------------------------
// (1) The parity gate.
// ----------------------------------------------------------------------------------------------

/// Strict mirror of `tests/unit/test_construct_edit_matrix.py`, INCLUDING its recorded-skip
/// short-circuit (a recorded `skip` never invokes the engine - test (2) covers those cells).
///
///   recorded          live skip            live fail             live ok
///   {"skip": ...}     pass (no engine)     pass                  pass
///   {"xfail": ...}    pass (skip wins)     pass (xfailed)        DIVERGENCE "now PASSES"
///   {"ok": ...}       pass (skip wins)     DIVERGENCE            pass
///
/// Every divergence is accumulated and asserted ONCE at the end - a per-cell `assert!` would hide
/// the other cells behind the first failure.
#[test]
fn edit_matrix_parity_with_python_expectations() {
    let runnable = runnable_increment_languages();
    let cases: Vec<CorpusCase> = corpus_cases()
        .into_iter()
        .filter(|case| runnable.contains(&case.language))
        .collect();
    assert_eq!(
        cases.len(),
        runnable.len(),
        "every runnable increment language must have a corpus dir"
    );

    let mut divergences: Vec<Divergence> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut executed: Vec<(String, &'static str)> = Vec::new();
    let mut live_census: HashMap<(String, &'static str), u32> = HashMap::new();
    let mut recorded_census: HashMap<(String, &'static str), u32> = HashMap::new();
    let mut sources: HashMap<String, String> = HashMap::new();

    for case in &cases {
        sources.insert(case.language.clone(), case.source.clone());
        write_dump(case);
        for verb in EDIT_VERBS {
            let cell = case.matrix.get(verb.name()).cloned().unwrap_or_default();
            let recorded = cell.recorded();
            if !matches!(recorded, Recorded::Skip(_)) {
                // Census over EXECUTED cells only, so `live skip` vs `recorded skip` compares
                // like with like instead of always reading 0 against the short-circuit count.
                *recorded_census
                    .entry((
                        verb.name().to_owned(),
                        match recorded {
                            Recorded::Ok => "ok",
                            Recorded::Xfail(_) => "xfail",
                            Recorded::Skip(_) => unreachable!(),
                        },
                    ))
                    .or_insert(0) += 1;
            }
            if let Recorded::Skip(_) = recorded {
                // Python `pytest.skip`s before touching the engine. Mirror it exactly here; the
                // skip audit (test 2) is what stops those cells from being invisible to Rust.
                continue;
            }
            let run = run_verb(&case.language, &case.filename, &case.source, verb);
            executed.push((case.language.clone(), verb.name()));
            *live_census
                .entry((verb.name().to_owned(), run.status.kind()))
                .or_insert(0) += 1;

            match (&recorded, &run.status) {
                // A runtime skip beats both a recorded ok and a recorded xfail (pytest.skip
                // returns before the xfail branch).
                (_, Status::Skip(_)) => {}
                (Recorded::Xfail(reason), Status::Fail(detail)) => {
                    if detail != reason {
                        notes.push(format!(
                            "NOTE {} [{}]: xfails for a DIFFERENT reason than recorded.\n       \
                             recorded: {reason}\n       live:     {detail}",
                            case.language,
                            verb.name()
                        ));
                    }
                }
                (Recorded::Xfail(reason), Status::Ok) => divergences.push(Divergence {
                    language: case.language.clone(),
                    verb,
                    recorded: recorded.clone(),
                    run,
                    filename: case.filename.clone(),
                    headline: format!(
                        "now PASSES -- remove its xfail from edit_matrix.expect.json (was: {reason})"
                    ),
                }),
                (Recorded::Ok, Status::Fail(detail)) => {
                    let detail = detail.clone();
                    divergences.push(Divergence {
                        language: case.language.clone(),
                        verb,
                        recorded: recorded.clone(),
                        run,
                        filename: case.filename.clone(),
                        headline: format!("recorded ok, Rust FAILS: {detail}"),
                    })
                }
                (Recorded::Ok, Status::Ok) => {}
                (Recorded::Skip(_), _) => unreachable!("short-circuited above"),
            }
        }
    }

    // Coverage assert BEFORE the results assert: a component that failed to instantiate produces
    // an Err at the mutation-diff step, which is past the tolerance boundary and already panics -
    // but a language dropped from enumeration would otherwise be invisible.
    let executed_languages: HashSet<&str> =
        executed.iter().map(|(lang, _)| lang.as_str()).collect();
    // A language whose every cell is a recorded skip (generic) legitimately executes nothing HERE;
    // test (2) is what covers it. So the expected set is "runnable languages with at least one
    // non-recorded-skip cell", and any language outside it must be wholly skip-recorded.
    let expected_languages: HashSet<&str> = cases
        .iter()
        .filter(|case| case.matrix.values().any(|cell| cell.skip.is_none()))
        .map(|case| case.language.as_str())
        .collect();
    assert_eq!(
        executed_languages, expected_languages,
        "every runnable increment language with a non-skip cell must have executed it"
    );
    for case in &cases {
        assert!(
            executed_languages.contains(case.language.as_str())
                || case.matrix.values().all(|cell| cell.skip.is_some()),
            "{} executed no cells but is not wholly recorded-skip - it was dropped silently",
            case.language
        );
    }
    let expected_cells: usize = cases
        .iter()
        .map(|case| {
            case.matrix
                .values()
                .filter(|cell| cell.skip.is_none())
                .count()
        })
        .sum();
    assert_eq!(
        executed.len(),
        expected_cells,
        "executed cell count must equal the non-recorded-skip cells of the runnable languages"
    );

    let mut report = String::new();
    report.push_str(&format!(
        "\n{}\nEDIT MATRIX PARITY -- {} divergence(s) across {} executed cells \
         ({} language(s) x {} verbs, {} cells short-circuited by a recorded skip)\n{}\n",
        "=".repeat(94),
        divergences.len(),
        executed.len(),
        cases.len(),
        EDIT_VERBS.len(),
        cases.len() * EDIT_VERBS.len() - executed.len(),
        "=".repeat(94),
    ));
    if !wasm_is_staged() {
        report.push_str(
            "  WASM-FREE MODE: no parser components are staged, so only the wasm-free dispatches\n\
             \x20 ran (python = certified native batch, generic = native text review). Stage\n\
             \x20 src/intentdiff/wasm (gitignored) or set INTENTUMDIFF_TEST_WASM_DIR for the rest.\n",
        );
    }
    for note in &notes {
        report.push_str(&format!("  {note}\n"));
    }
    for (index, divergence) in divergences.iter().enumerate() {
        let source = sources
            .get(&divergence.language)
            .map(String::as_str)
            .unwrap_or("");
        report.push_str(&render_divergence(index, divergences.len(), divergence, source));
    }
    report.push_str(&render_census(&live_census, &recorded_census));
    println!("{report}");

    assert!(
        divergences.is_empty(),
        "{} edit-matrix divergence(s) between the Rust harness and the recorded Python \
         expectations:{report}",
        divergences.len()
    );
}

// ----------------------------------------------------------------------------------------------
// (2) The skip audit.
// ----------------------------------------------------------------------------------------------

/// Test (1) never invokes the engine for a recorded-skip cell, so without this a fifth of the
/// matrix is invisible to Rust. Re-run every recorded skip and classify:
///   STALE  - now `ok`   (the ratchet is lying; the cell should be pinned)
///   MASKED - now `fail` (a real failure hiding behind a skip)
///   honest - still skips
/// Both non-honest sets must be empty. This is a GATE, not advisory.
#[test]
fn edit_matrix_skip_audit_matches_python() {
    let runnable = runnable_increment_languages();
    let mut stale: Vec<String> = Vec::new();
    let mut masked: Vec<String> = Vec::new();
    let mut honest = 0usize;
    let mut drifted: Vec<String> = Vec::new();

    for case in corpus_cases()
        .into_iter()
        .filter(|case| runnable.contains(&case.language))
    {
        for verb in EDIT_VERBS {
            let cell = case.matrix.get(verb.name()).cloned().unwrap_or_default();
            let Recorded::Skip(reason) = cell.recorded() else {
                continue;
            };
            let run = run_verb(&case.language, &case.filename, &case.source, verb);
            match &run.status {
                Status::Ok => stale.push(format!(
                    "STALE  {} [{}] now passes; recorded skip was {}",
                    case.language,
                    verb.name(),
                    py_repr(&reason)
                )),
                Status::Fail(detail) => masked.push(format!(
                    "MASKED {} [{}] now FAILS ({detail}); recorded skip was {}",
                    case.language,
                    verb.name(),
                    py_repr(&reason)
                )),
                Status::Skip(detail) => {
                    honest += 1;
                    if detail != &reason {
                        drifted.push(format!(
                            "NOTE   {} [{}] still skips, but for a different reason.\n         \
                             recorded: {reason}\n         live:     {detail}",
                            case.language,
                            verb.name()
                        ));
                    }
                }
            }
        }
    }

    println!(
        "\nSKIP AUDIT -- {honest} honest, {} stale, {} masked\n{}",
        stale.len(),
        masked.len(),
        drifted.join("\n")
    );
    assert!(
        stale.is_empty() && masked.is_empty(),
        "recorded skips are no longer honest. A STALE skip is a ratchet that has rotted (the \
         engine improved after the manifest was generated and nothing re-ran the cell); a MASKED \
         skip is a live failure hiding behind one. Re-derive the affected cells with \
         INTENTUMDIFF_EDIT_MATRIX_DUMP=<dir> and commit the delta, or fix the engine.\n{}\n{}",
        stale.join("\n"),
        masked.join("\n")
    );
}

// ----------------------------------------------------------------------------------------------
// (3) Census: every corpus language has a dispatch. UNGATED - needs no wasm at all.
// ----------------------------------------------------------------------------------------------

#[test]
fn edit_matrix_every_corpus_language_has_a_dispatch() {
    let cases = corpus_cases();
    let languages: HashSet<String> = cases.iter().map(|c| c.language.clone()).collect();

    // A dir that fails the `playground.expect.json` / single-`playground.*` rules is dropped
    // SILENTLY by Python. Count the dirs that carry an edit matrix and assert none vanished.
    let matrix_dirs = fs::read_dir(corpus_dir())
        .expect("corpus dir")
        .flatten()
        .filter(|entry| entry.path().join("edit_matrix.expect.json").is_file())
        .count();
    assert_eq!(
        cases.len(),
        matrix_dirs,
        "a corpus dir carrying edit_matrix.expect.json was silently dropped by the loader"
    );
    assert_eq!(
        cases.len(),
        73,
        "the corpus is 73 languages (74 entries minus GENERATION_REPORT.json); a change here is a \
         deliberate act - update this number in the same commit that adds the language"
    );
    // Every dir name is also its language id (the loader keys off the manifest, so this is a real
    // check, not a tautology).
    for case in &cases {
        assert_eq!(
            case.dir_name, case.language,
            "corpus dir name and manifest language must agree"
        );
        assert!(
            !case.filename.is_empty(),
            "{}: manifest filename drives language detection and cannot be empty",
            case.language
        );
    }

    // No dead rows in the hard-coded overrides.
    for (language, _) in [
        ("c", ()), ("cpp", ()), ("javascript", ()), ("typescript", ()), ("tsx", ()),
        ("hcl", ()), ("m", ()), ("wast", ()), ("wat", ()), ("databricks", ()),
        ("databricks-workflow", ()), ("python", ()), ("generic", ()),
    ] {
        assert!(
            languages.contains(language),
            "dispatch table has a row for {language:?} which is not a corpus language"
        );
    }

    // Cross-check against parser_manifest.json wherever it HAS an entry, so table drift is caught.
    let manifest_path = staged_wasm_dir().join("parser_manifest.json");
    if manifest_path.is_file() {
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest"))
                .expect("parse parser_manifest.json");
        let parsers = manifest["parsers"]
            .as_object()
            .expect("parser_manifest.json has a parsers object");
        let mut absent: Vec<&str> = Vec::new();
        for case in &cases {
            match parsers.get(&case.language) {
                None => absent.push(&case.language),
                Some(entry) => {
                    let expected = entry["wasm"].as_str().unwrap_or_default();
                    if let Some(mapped) = component_for(&case.language).component() {
                        assert_eq!(
                            mapped, expected,
                            "dispatch table drifted from parser_manifest.json for {}",
                            case.language
                        );
                    }
                }
            }
        }
        absent.sort();
        // PINNED PRODUCT GAP (not a test-only concern): these languages have no manifest entry, so
        // `parse_to_tree`, `live_diff_contents_impl` and the NATIVE LIVE-SERVER cannot resolve a
        // parser for them and fall back. Regenerating via scripts/gen_parser_manifest.py is the
        // fix; the harness works around it with the hard-coded table above.
        assert_eq!(
            absent,
            vec!["cmake", "databricks", "gomod", "ini", "make", "proto", "toml", "wast"],
            "the set of corpus languages missing from parser_manifest.json changed"
        );
    }
}

// ----------------------------------------------------------------------------------------------
// (4) Every component the increment maps is staged.
// ----------------------------------------------------------------------------------------------

#[test]
fn edit_matrix_every_component_is_staged() {
    if !wasm_is_staged() {
        println!(
            "SKIPPED: no parser components staged in {}. Set INTENTUMDIFF_TEST_WASM_DIR or stage \
             src/intentdiff/wasm (gitignored) to run the wasm dispatches.",
            staged_wasm_dir().display()
        );
        return;
    }
    let mut missing: Vec<String> = Vec::new();
    for language in INCREMENT_LANGUAGES {
        if let Some(name) = component_for(language).component() {
            let path = component_path(&name);
            if !path.is_file() {
                missing.push(format!("{language} -> {}", path.display()));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "parser components are staged, but these mapped ones are absent (a language would be \
         silently dropped from the matrix):\n  {}\nSet INTENTUMDIFF_TEST_WASM_DIR to a complete \
         wasm dir.",
        missing.join("\n  ")
    );
}

// ----------------------------------------------------------------------------------------------
// (5) The tree dump - the issue's first deliverable.
// ----------------------------------------------------------------------------------------------

/// Prints the semantic tree the engine builds for each increment fixture (language, node_type,
/// label, span, structural_hash per node) plus the entities/literals the matrix discovers from it.
/// Run with `--nocapture`; `INTENTUMDIFF_TREE_DUMP=<language>` narrows it to one language (and
/// accepts any corpus language, not just the increment ones).
#[test]
fn edit_matrix_tree_dump() {
    let selected = ["INTENTUMDIFF_TREE_DUMP", "INTENTDIFF_TREE_DUMP"]
        .iter()
        .find_map(|name| std::env::var(name).ok());
    let languages: Vec<String> = match &selected {
        Some(language) => vec![language.clone()],
        None => runnable_increment_languages(),
    };
    for language in languages {
        match dump_fixture_tree(&language) {
            Ok(dump) => println!("{dump}"),
            Err(exc) => panic!("tree dump for {language} failed: {exc}"),
        }
    }
}
