# External correctness benchmarks: 0.0.3 foundation

This tranche adds independent source evidence and fixes C/C++ character-literal
projection. It does not certify the whole release. Work targets
`release/v0.0.3-rc`; `main` and published packages remain release baselines.

The ledger accounts for all 74 routed language IDs. The executable pilot contains
the first twelve HumanEvalPack tasks in Python, JavaScript, Java, Go, C++ and Rust,
plus identity, formatting, rename and focused character-literal controls. See the
[fixture README](../tests/fixtures/external/README.md) for immutable source hashes,
licensing declarations, admission gaps and the original CPP/11 behavior witness.
Translated tasks are related observations, not independent portfolio problems.

There are 167 executions: 96 admitted checks and 71 repair seeds without admitted
change-fact oracles. Seeds are executed and reported as unadmitted; they cannot
inflate an accuracy denominator. Only CPP/11 currently has an admitted upstream
repair oracle. The other repair pairs still need independent evidence review.

## Reproduce the Linux gate

Use Python 3.12 and Rust 1.95.0. The setup phase acquires inputs; execution reads
only local fixtures and components. The workflow
`External source correctness (Linux)` provides the complete acquisition commands.
Its immutable inputs are in `scripts/external_benchmark_inputs.lock.json`.

The published wheel supplies existing component bytes. Each is independently
checked against the committed registry. C/C++, JavaScript/TypeScript, JSON, SQL,
YAML, ADF and Databricks come from the exact successful CI artifacts recorded in
the schema-2 lock. Both source commits and component hashes must agree with that registry.
Acquisition verifies the artifact ID, successful source run, archive digest,
single flat archive member and component digest. The same acquisition helper
serves Tier-C provisioning; locked components never fall back to wheel or latest
build bytes. Redirects cannot forward credentials across hosts or downgrade HTTPS.

For offline staging, supply the locked archives as `<artifact_id>.zip` in
`--artifacts-dir` and omit `--download-artifacts`. The stager rejects dirty
registry inputs, missing components, stale output and identity mismatches. It
publishes a complete directory atomically only after every input validates.
The manifest distinguishes registry-matched wheel mirrors from locked archives
and records their source refs, artifact IDs, archive hashes and registry commit.
Artifact expiration is an explicit acquisition failure; there is no fallback to
a newer build. CI retains the normal report/manifest identities with its results.

After staging to `build/benchmark-inputs`, run from the repository root:

```sh
python scripts/import_external_benchmarks.py --validate
python -m pytest tests/tooling -q
cargo test --locked --manifest-path crates/rust-core-host/Cargo.toml --no-default-features --example external_benchmarks
cargo build --locked --manifest-path crates/rust-core-host/Cargo.toml --no-default-features --lib --example external_benchmarks
crates/rust-core-host/target/debug/examples/external_benchmarks --fixtures tests/fixtures/external --parsers build/benchmark-inputs --report build/benchmarks/results.json --strict
```

All paths are relative to the process working directory. `--core-library PATH`
selects the C ABI library whose digest is retained for downstream wheel identity
checks; otherwise it is found beside the built example. The Rust runner still
executes the in-process public API, and records its own executable/source identity.

Exit 2 denotes invalid input or acquisition/initialization failure. Exit 1 denotes
recorded runtime failures or failed mandatory checks; `--strict` also rejects any
admitted optional accuracy failure. Every case and every inventory language has a
row. Runtime/resource failures and successful judged accuracy have separate counts.
Zero-case and unadmitted rates are null with an explicit reason.

Exact source hashes and raw review results are retained. Determinism compares
semantic output while excluding documented timing, cache-hit and measured fuel
telemetry; raw telemetry remains in the report. Source-span and expected-fact
checks do not use current engine output as a golden answer. Byte-identity controls
leave `is_style_only` unasserted because its no-edit meaning is not defined
consistently across routes; they still require no semantic change or invented edits.

## Installed binding acceptance

Build and install a candidate wheel with the same core and verified components,
then run the Python repository's `scripts/check_external_binding.py` from outside
its checkout. Supply `--core-report`, `--fixtures`, `--parsers` and `--report`.
Each admitted case is independently checked through Rust, direct C ABI and public
Python; source/component hashes, review options and the actual installed ABI
library digest must match. Parity alone is not correctness. The gate refuses
editable/source-checkout imports and validates every native row before filtering
unadmitted cases, so a failed seed execution cannot disappear.

The remaining 0.0.3 programme includes external-oracle admission for the other
languages/repairs, broader semantic defects, fuzz/load acceptance, supported-platform
certification and coordinated release automation. These remain open; no release is
published by this work.

## Admission ledger and scorecard (schema 2, core #268)

The schema-2 ledger at `tests/fixtures/external/admission/ledger.json` is the common
contract for the language-family corpora (#262–#267). It reuses the pilot's retained
bytes, routing inventory (`inventory.json`) and all-language source selection
(`ledger.json`) by path and SHA-256; only new derived/handwritten bytes live under
`admission/cases/`, and executed witness records under `admission/witnesses/`.

**Suite record**: `suite_id`, `name`, `version`, `url`, immutable 40-hex `revision`
(handwritten suites: `in-repository` plus per-file hashes), `licence`,
`licence_evidence`, `attribution`, `evidence_class`, and `acquisition` (method, exact
files with SHA-256, and every verification step, including what could not be verified).

**Discovery row** per suite × language × lane: `discovered` upstream tasks, `eligible`
under a stated `eligibility_rule`, the imported rows and the holdout policy.

**Case record**:

| Field | Meaning |
|---|---|
| `upstream` | original case/task ID, row index, the upstream fields used, and the retained record (path + SHA-256) |
| `old` / `new` | exact source bytes (path + SHA-256) |
| `derivation` | ordered history; first step `verbatim_upstream`, `derived_transformation`, `identity_control` or `handwritten` |
| `language_id`, `dialect`, `filename` | registered route and the toolchain dialect the oracle used |
| `class` | `upstream_gold`, `reviewed_external_derived`, `external_seed_only` or `handwritten_regression` |
| `lane` | `parser_fidelity`, `change_understanding` or `robustness_scaling` |
| `behavior` | `claim` (`changed` / `preserved` / `unknown`) and its `assumptions` — separate from IntentumDiff classification |
| `expected` | `classification` (each of `has_semantic_changes`, `is_style_only`, `changes_empty`, `completeness`; `null` = unconstrained), `required`, `alternatives`, `forbidden` |
| `oracle` | `basis` (`executable_witness`, `curated_annotation`, `specification_review`, `byte_identity`, `source_review`, `candidate_only`), witness record, notes |
| `admission` | `admitted` / `candidate` / `rejected` with the review reason |
| `licence`, `attribution`, `known_issue` | per-case provenance and any linked bug |

Required results are `edit_region` (a reported change of one of `change_types_any_of`
must touch an independently computed byte delta) or `change_fact` (labels, optionally
as descendant evidence). An `alternatives` entry is a complete equally valid result set.
Forbidden results are `change_type`, `style_only`, `no_changes` and `fallback`.

The validator (`python scripts/external_ledger.py validate`) enforces, among others:
safe relative paths and SHA-256 for every byte; known classes/lanes/bases; an admitted
`upstream_gold` or `reviewed_external_derived` case needs an executed witness whose
verdict supports its behavior claim (byte identity alone does not promote a case);
`upstream_gold` bytes are verbatim upstream; seed-only and candidate-only cases cannot be
admitted; unadmitted cases carry `expected: null`; handwritten cases belong to a
handwritten suite and never enter an external-gold denominator.

Witnesses: `scripts/humanevalpack_witness.py` runs the retained upstream test against the
exact case bytes with explicit toolchains (Python, Node.js, javac/java, Go + testify,
g++/gcc, cargo + rand/regex/md5), offline, and writes the record the ledger pins.
Repairs need old to fail and new to pass; preservation controls need both to pass. A
harness/compile failure is `inconclusive`, a missing toolchain `blocked`; neither admits.

`scripts/build_external_admission.py` rebuilds the ledger deterministically from those
inputs (`--check` fails on a stale ledger). `scripts/run_external_ledger.py` executes
admitted cases through the production C ABI by reusing `assess_languages.py` workers
(routing, fallback, tree/range, group-reference, completeness and determinism checks),
then judges each review against the ledger. `scripts/external_ledger.py scorecard` emits
rows per language × suite × class × lane with discovered, eligible, admitted, executed,
passed, failed, unsupported, skipped and blocked counts. Every rate carries numerator and
denominator; zero denominators are `null` with a reason. Every registered language has
lane entries; a lane with no executed admitted external case is `gap` (or `blocked`),
never 100%. No pooled average is computed.

```sh
python scripts/external_ledger.py validate
python scripts/build_external_admission.py --check
PYTHONPATH=<intentumdiff-python>/src python scripts/run_external_ledger.py \
  --parsers build/benchmark-inputs --native-library <libintentumdiff_rust_core.so> \
  --report build/benchmarks/ledger-results.json \
  --baseline docs/evidence/2026-11-17-external-scorecard/results.json
python scripts/external_ledger.py scorecard --results build/benchmarks/ledger-results.json \
  --out-json build/benchmarks/scorecard.json --out-md build/benchmarks/scorecard.md
```
