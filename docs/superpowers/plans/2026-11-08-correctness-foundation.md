# IntentumDiff 0.0.3 correctness foundation

Approved plan: implement an engine-owned external benchmark ledger covering all 74 routed IDs, the first 12 HumanEvalPack tasks in each of Python/JavaScript/Java/Go/C++/Rust (72 repair pairs plus 72 identity controls), independently reviewed formatting/rename controls, and C/C++ character-literal retention. Required platform: Linux. Wider language execution, supported-platform certification, fuzz/load testing and release automation remain later work. No publication.

## Tasks

1. Establish release/v0.0.3-rc and feature branches from main; pin Rust 1.95.0 and reconcile the parser SDK lock.
2. Add immutable external source records, source bytes, required/forbidden expectations, all-74-language admission ledger and reproducible importer under tests/fixtures/external and scripts. Distinguish upstream gold/reviewed external-derived/seed-only/handwritten cases. Use core issues #261 and #268 as specifications.
3. Add a Rust benchmark example and integration tests, using existing api::review_sources; validate provenance, source hashes, tree spans, deterministic output and independent expectations; emit JSON results with per-language denominators. Infrastructure failures fail; accuracy failures remain explicit, not rewritten expectations.
4. Fix cpp-parser #5: atomic complete character-literal payload and hash, preserving prefix/escapes and byte spans. Test actual component through production host and Rust/C ABI/installed Python routes. Vet parser artifact before coordinated registry and consumer pin updates.
5. Wire a required offline Linux gate and documentation. Review all changes with a separate agent; resolve actionable findings and rerun relevant tests. Prepare reviewable PRs without merging or publishing.

## Shared fixture contract (schema_version 1)

`tests/fixtures/external/manifest.json`: {schema_version:1, sources:[...], cases:[...]}. Source: {id, url, revision, license, files:[{path,sha256}], ...provenance}. Case: {id, language_id, filename, class, family, old_path, new_path, old_sha256, new_sha256, source_id, upstream_task_id, oracle:{basis,behavior,notes,...}, expected:{has_semantic_changes,is_style_only,required_changes:[{change_type,old_label?,new_label?}],forbidden_change_types:[...]}, required:bool}. Paths are relative to fixture root; no traversal. Allowed classes: upstream_gold, reviewed_external_derived, external_seed_only, handwritten_regression. Behavior: changed/preserved/unknown; classification remains separate. Required facts may be empty for unadmitted seed cases, whose expected field is null and required is false. Any fixture promoting claims needs actual independent evidence. All pilot pairs are executed even if exact intent is not admitted.

`tests/fixtures/external/ledger.json`: {schema_version:1, languages:[{language_id, component, source_url, evidence_class, admission_status, reason, ...}]}; every one of the 74 current routed IDs appears once.

Example runner: `cargo run --locked --manifest-path crates/rust-core-host/Cargo.toml --example external_benchmarks -- --fixtures tests/fixtures/external --parsers build/wasm --report build/benchmarks/results.json`; CLI fixture/parser/report paths resolved from current working directory, not compile-time cwd. Staged parser_manifest.json is mandatory. Default exits nonzero on infrastructure failures or failed required cases; --strict additionally fails any admitted accuracy failure. Report every inventory language, zero-case rates null with a reason. Exact run identities, component sha256, interface and options retained.
