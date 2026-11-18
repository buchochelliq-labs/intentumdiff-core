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
checked against the committed registry. Every component whose registry pin the
wheel does not mirror (C/C++, JavaScript/TypeScript, JSON, SQL, YAML, ADF,
Databricks, and the 36 others re-pinned by registry #20) comes from the exact
successful CI artifacts recorded in the schema-2 lock. Both source commits and component hashes must agree with that registry.
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
| `expected` | `classification` (each of `has_semantic_changes`, `is_style_only`, `changes_empty`, `completeness`; `null` = unconstrained), `required`, `alternatives`, `forbidden`, and on parser-fidelity cases `parse` (see below) |
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
python scripts/import_external_benchmarks.py --validate
python scripts/external_ledger.py validate
python scripts/structured_family.py build --check      # the #265 fragment is current
python scripts/markup_family.py build --check          # the #263 fragment is current
python scripts/build_external_admission.py --check     # the ledger is current
PYTHONPATH=<intentumdiff-python>/src python scripts/run_external_ledger.py \
  --parsers build/benchmark-inputs --native-library <libintentumdiff_rust_core.so> \
  --report build/benchmarks/ledger-results.json \
  --baseline docs/evidence/2026-11-17-external-scorecard/results.json
python scripts/external_ledger.py scorecard --results build/benchmarks/ledger-results.json \
  --out-json build/benchmarks/scorecard.json --out-md build/benchmarks/scorecard.md
```

## Language-family contributions

Each family task (#262–#267) contributes its suites, discovery rows, cases and optional
`blocked_acquisitions` to the one ledger. `build_external_admission.py` supports two mechanisms and merges
them after the first slice in a fixed order, so independently added families rebuild one deterministic
ledger:

1. **Code families**: `scripts/external_families/*.py`, in sorted file-name order. Each module exposes
   `contribute(ctx)` and returns `{'suites', 'discovery', 'cases'}`, built with the builder's own helpers.
   Upstream bytes are vendored under `tests/fixtures/external/upstream/<suite_id>/`, and derived bytes go
   under `admission/cases/<language>/`. #262 and #267 use this mechanism.
2. **Fragment families**: `tests/fixtures/external/admission/families/*.json`, in sorted file-name order.
   The family's own deterministic tool generates each fragment and checks it with `--check`; the builder
   merges it verbatim. #265 and #263 use this mechanism.

The ledger and each fragment are deterministic build outputs, and both stay committed. Every consumer
reads the committed ledger directly: the validator, the runner, the witness tools and the tooling tests.
The CI baseline and the evidence also pin its SHA-256. The `--check` gates (`structured_family.py build
--check` and `markup_family.py build --check` for the fragments, then `build_external_admission.py --check`,
which also re-runs every code module and refuses to rewrite its derived case bytes) prove that the committed
files match their inputs.

A blocked acquisition (the ledger's top-level `blocked_acquisitions`) names the language, suite, lanes and reason. A blocked lane is shown as BLOCKED with
that reason. It is never shown as a gap or a pass.

## The parser-fidelity lane

A parser-fidelity case judges one document: the verbatim upstream bytes on both sides (old == new). It
carries `expected.parse`, which has one schema for both families and up to two independent claims:

| Field | Claim |
|---|---|
| `accepts` (bool, #265) | Conformance. `true` = must accept, `false` = must reject. |
| `statement_inventory` (`{count, starts, evidence}` or `null`, #262) | The reference engine's top-level statements in the `side` document. |
| `side` (`old` / `new`) | The judged document. It is required without `accepts`; with `accepts` it defaults to `old`. |

The validator requires `accepts` or `side` on every admitted parser-fidelity case. A must-reject case
cannot carry an inventory or constrain a review. The runner (`run_external_ledger.py`) applies these
rules:

- **Must-accept** (`accepts: true`): the case fails on any explicit fallback, parse diagnostic, tree or
  determinism error, or invented change. A rejected valid document is a failure, not `unsupported`.
- **Must-reject** (`accepts: false`): the case passes only when the C ABI reports the problem, through an
  explicit fallback/error or `parse_errors`. Silent acceptance fails.
- **Statement inventory without `accepts`**: the reference acceptance is a precondition, so a fallback
  leaves the case `unsupported`. Otherwise `external_ledger.judge_parse` compares the inventory with the
  parse tree:
  - The top-level units are the root's children.
  - A node that spans more than one reference start is a container and is replaced by its own children.
  - The unit count and each unit's start byte must match the reference.

  A `null` inventory only checks acceptance: no parse errors, no fallback and no invented changes. The
  inventory comes from the reference engine or parser, never from IntentumDiff.

Parser-fidelity rows report the valid-acceptance and invalid-rejection rates; an inventory case counts
as a must-accept document. Change-fact rates for these rows are N/A.

### #262 (SQL and data-platform: sql, tsql, plsql, m, adf)

The family module is `scripts/external_families/family_262_sql_data.py`. Its witnesses are written by
`scripts/data_family_witness.py ledger`, which reads each case's `witness_plan`. It needs:

- a PostgreSQL server with `logging_collector=on`, `log_destination=csvlog`, `log_statement=all` and
  `log_min_error_statement=error`, reached through `PGHOST`, `PGPORT` and `PGUSER`, with `--csvlog`;
- `dotnet` and the built `scripts/witness_tools/scriptdom` tool, passed as `--tool`;
- `node` with `@microsoft/powerquery-parser@4.0.1` in `--node-path`.

CI does not rerun these witnesses. It validates the committed records and rebuilds the ledger with
`--check`.

### #265 (structured: yaml, json, toml, xml, ini, proto, graphql)

The family contributes the fragment `tests/fixtures/external/admission/families/structured-265.json`.

```sh
python scripts/acquire_structured_benchmarks.py           # network: raw.githubusercontent.com at pinned commits
STRUCTURED_REF_PYTHON=<python with ruamel.yaml 0.18.16 + jsonpatch 1.33> \
STRUCTURED_REF_NODE_MODULES=<node_modules with graphql@16.15.0 + @bufbuild/buf@1.75.0> \
  python scripts/structured_family.py witness             # executes the reference oracles
python scripts/structured_family.py build [--check]       # offline, deterministic fragment
python scripts/build_external_admission.py [--check]
```

Parser-fidelity cases carry `expected.parse.accepts`. Change-understanding upstream pairs are admitted
as `upstream_gold`. They come from buf `previous/`→`current/`, graphql-js `oldSchema`/`newSchema`
template literals (extracted byte-exact) and YAML `in.yaml`→`out.yaml`. Every other pair is
`reviewed_external_derived`, with its derivation recorded on the case. Derived pairs come from JSON Patch
`doc`/`expected` serialisations, key reorders, re-serialisations, scalar value/type edits, comments,
delimiter spacing and literal respelling. Each claim is witnessed by a reference implementation run
outside IntentumDiff:

- typed value equality: json, tomllib, ruamel.yaml, configparser;
- buf image equality and `breaking`, reproducing the exact upstream annotation set;
- graphql-js `findBreakingChanges`/`findDangerousChanges`, matching the upstream assertions, and sorted
  `printSchema` equality.

The W3C XML suite is a blocked acquisition (www.w3.org is unreachable), so `xml` is BLOCKED.

### #263 (web, markup and embedded: markdown, mdx, html, css, scss, vue, svelte, astro, latex, asciidoc, po, postscript, generic)

The family contributes the fragment `tests/fixtures/external/admission/families/markup-263.json`, the second
fragment family; it reuses the #265 expectation helpers and the shared parser-fidelity adapter.

```sh
python scripts/acquire_markup_benchmarks.py               # network: raw.githubusercontent.com at pinned commits, npm tarballs
MARKUP_REF_NODE_MODULES=<node_modules with commonmark@0.31.2 sass@1.108.2 svelte@5.57.3 @vue/compiler-sfc@3.5.45
  vue@3.5.45 @astrojs/compiler@4.0.0 @mdx-js/mdx@3.1.1> \
MARKUP_REF_PYTHON=<python with html5lib 1.1, tinycss2 1.5.1, cmarkgfm 2025.10.22> \
MARKUP_REF_ASCIIDOCTOR=<asciidoctor 2.0.26> \
  python scripts/markup_family.py witness                 # also needs GNU msgfmt and TeX Live latex on PATH
python scripts/markup_family.py build [--check]           # offline, deterministic fragment
python scripts/build_external_admission.py [--check]
```

Parser-fidelity cases are verbatim upstream documents (CommonMark/GFM examples, WPT html5lib `#data`,
css-parsing-tests inputs and Vue/Astro test sources are extracted byte-exact by each suite's own rule).
Each claim is confirmed by a reference: the upstream expected HTML/tree/AST/CSS must be reproduced
(commonmark.js, cmark-gfm, html5lib, tinycss2, Dart Sass), or the compiler/converter must accept it
(Svelte, Vue, Astro, MDX, Asciidoctor without warnings, `msgfmt --check`, pdfTeX). CommonMark cases also
carry the commonmark.js top-level block inventory (start byte of each block's first line). Change pairs
are Svelte's own migrate input/output (`upstream_gold`, witnessed by equal server-rendered HTML) and
bounded `reviewed_external_derived` edits (24 per derivation and suite) witnessed by rendered HTML,
html5lib trees, tinycss2 ASTs, compiled CSS, SSR HTML, compiled modules, `.mo` bytes or DVI bytes.
`postscript` (Ghostscript samples are AGPL-3.0) and `generic` (WikiAtomicEdits data answers 403) are
blocked acquisitions.

### #267 (build, configuration and infrastructure: dockerfile, hcl, gomod, gitignore, cmake, make, puppet)

The family module is `scripts/external_families/family_267_build_infra.py`. Its `plan()` lists every case and is
shared with the witness tool. `bash` and `powershell` are left to #266.

```sh
python scripts/acquire_build_infra_benchmarks.py          # network: Go module zips (sum.golang.org) and raw files
(cd scripts/witness_tools/buildinfra && go build -o "$BUILDINFRA_TOOL" .)   # BuildKit v0.17.3, hcl v2.23.0, x/mod v0.22.0
GEM_HOME=<gems with puppet 8.10.0> BUILDINFRA_TOOL=... \
  python scripts/build_infra_witness.py extract           # GNU Make harness makefiles + puppet spec manifests
GEM_HOME=... BUILDINFRA_TOOL=... python scripts/build_infra_witness.py witness
python scripts/build_external_admission.py [--check]
```

The witness tool also needs go1.24.7, git 2.43.0, cmake 3.28.3, GNU Make 4.3 and Perl. A missing tool
produces a `blocked` record, and a blocked record never admits a case.

The oracles run outside IntentumDiff:

- **dockerfile:** BuildKit `parser.Parse`, which must reproduce each upstream `result` dump, and
  `instructions.Parse` typed instruction lists for pairs.
- **hcl:** `hclsyntax.ParseConfig` acceptance, and an information-model projection for pairs (`hclwrite.Format`
  makes the canonical-format pairs).
- **gomod:** `modfile.Parse` and `go mod edit -json`.
- **gitignore:** git reproducing the t0008 expectations, and `check-ignore` sets over a fixed universe for pairs.
- **cmake:** the upstream RunCMake harness, and configure-output equality for pairs.
- **make:** the upstream `run_make_tests.pl` harness, and make-output equality for pairs.
- **puppet:** `Parser#parse_string` as the specs call it, and normalised catalog equality for pairs.

IaC-Eval and Shipwright are blocked acquisitions.

### Combined results

The first slice, #262, #265, #263 and #267 share one ledger and one runner report. The combined results,
measured on the components locked to registry `eb02686` (#21), with minimised failures and blocked sources
for every family, are in `docs/evidence/2026-11-17-external-scorecard/README.md`. Its `results.json` is the
CI regression baseline.
