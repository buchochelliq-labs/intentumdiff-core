# External-benchmark scorecard: first slice, 2026-11-17

This is the first scorecard produced under the core #268 contract (schema-2 admission ledger). It covers
the first #261 slice: HumanEvalPack tasks 0–11 in Python, JavaScript, Java, Go, C++ and Rust, plus
handwritten C and C++ character-literal regressions. **It does not certify any language.** 68 of the 74
registered IDs have no executed external case and appear as `GAP` in every lane.

**Update (#262 SQL and data-platform family, same day).** The ledger now also carries the #262 family:
PostgreSQL regression (`sql`), Microsoft SqlScriptDOM (`tsql`), Microsoft powerquery-parser (`m`) and
Azure Data Factory samples (`adf`), plus utPLSQL seeds (`plsql`). With it, 64 of the 74 IDs remain gaps.
`results.json` and the scorecard were regenerated from the combined ledger; see the
[#262 section](#262-sql-and-data-platform-family) below.

Files:

- `scorecard.md` and `scorecard.json`: rows for each language × suite × class × lane, with discovered,
  eligible, admitted, executed, passed, failed, unsupported, skipped and blocked counts. Every one of
  the 74 IDs has an entry.
- `results.json`: the runner report, with a compact review summary for each case. It is also the
  reviewed baseline for the CI regression gate.
- `parser_manifest.json`: the exact staged component identities.
- `adversarial-old-cpp-component.json`: the guard check described below.

## Identity

- Core `74618c311836983b4f60ae2d8389edfdd14f7793` (`release/v0.0.3-rc`, crates clean).
- C ABI library: `cargo build --locked --release --lib` with Rust 1.95.0, `CARGO_PROFILE_RELEASE_DEBUG=0`.
  Its SHA-256 is `ced9e0e1…598b`; the full value is in `results.json`.
- Components were staged with `scripts/stage_external_benchmark_inputs.py` from:
  - registry commit `28f5fce5…`;
  - the published `intentumdiff-python==0.0.2b2` wheel as byte mirror;
  - the seven locked CI artifacts in `scripts/external_benchmark_inputs.lock.json`, downloaded and
    verified by artifact ID, archive digest and component digest.
- The `intentumdiff.rust_core` C ABI transport came from intentumdiff-python `0dd7e50` on `PYTHONPATH`.

## HumanEvalPack acquisition

The dataset is `bigcode/humanevalpack` at revision `9a41762f73a8cb23bb5811b73d5aab164efcf378`, licensed
MIT according to the dataset card's `license: mit`. Attribution: BigCode / OctoPack, Muennighoff et al.,
arXiv:2308.07124.

**Blocked:** the raw Parquet bytes could not be fetched again on 2026-11-17.

- huggingface.co is not reachable from this sandbox.
- The Hugging Face MCP `hf_fs` tool refuses to `cat` binary files (`HF_FS_TEXT_ONLY`), and its `stat`
  exposes no LFS SHA-256.
- The dataset viewer preview timed out.

The ledger therefore uses the revision-pinned Parquet bytes that were downloaded on 2026-11-08 and are
already retained in the repository. Their SHA-256 values are pinned in
`scripts/import_external_benchmarks.py`. No other source was substituted.

What the MCP did verify at the pinned revision:

- The file listing contains only the six `*/test-00000-of-00001.parquet` files, `README.md` and
  `.gitattributes`, so there is no separate LICENSE file.
- Each Parquet byte size equals the retained file's size (cpp 193981, go 198394, java 210440,
  js 194044, python 191279, rust 168464).
- `README.md` is 12521 bytes, and its `dataset_info.download_size` fields equal those six sizes.

A content-hash re-verification against the Hub is still open.

## Witness executions

`scripts/humanevalpack_witness.py` ran the retained upstream `test` field against the exact admitted
bytes. It ran offline, with a 60 s per-step timeout. Old and new sides were run separately, giving 164
witness records.

| Config | Toolchain | Harness |
|---|---|---|
| python | Python 3.13.18 | `python3 -I` |
| js | Node.js v22.23.4 | `console.assert` output counts as a failure |
| java | javac/OpenJDK 21.0.12.1 | `java -ea Main` |
| go | go1.24.7 | `go test -vet=off`; testify v1.8.4 module |
| cpp | g++ 13.3.0 | `-std=c++17` |
| rust | cargo/rustc 1.97.0 | `cargo test`; rand 0.8.8, regex 1.13.1, md5 0.7.0, for the upstream `use` lines |
| c | gcc 13.3.0 | `-std=c11`; inline witness for the minimal literal case |

**158 witnesses support their claim.** Each repair fails or times out on the buggy side and passes on
the canonical side. Each preservation control passes on both sides. For a public rename, callers follow
the new name, so the new side runs with the entry point renamed in the test.

**6 Go witnesses are inconclusive** because they do not compile. Those cases stay candidates and are
not admitted:

- Go tasks 02 and 04: the upstream test calls `math.*`, but `test_setup` does not import `math`.
- Go task 10: the case bytes, built from declaration + solution, omit the `IsPalindrome` helper.
  That helper exists only in the upstream `prompt`. The case needs a re-import before admission.

## Admitted cases (165 executed through the C ABI)

| Language | upstream_gold | reviewed_external_derived | handwritten_regression | candidates |
|---|---:|---:|---:|---:|
| python | 12 | 15 | – | – |
| javascript | 12 | 15 | – | – |
| java | 12 | 15 | – | – |
| go | 9 | 12 | – | 6 |
| cpp | 12 | 15 | 5 | – |
| rust | 12 | 15 | – | – |
| c | – | – | 4 | – |

- **upstream_gold:** verbatim upstream repair pairs whose witness was executed.
- **reviewed_external_derived:** 12 identity controls, 1 formatting control, 1 public-rename control
  and 1 new local-identifier rename per language.
- **handwritten_regression:** the minimised `char f(){return '0';}` → `'1'` case for C and C++, plus
  the pilot's ordinary, escape, position and prefix literal regressions.

Handwritten cases never enter a lane rate.

## Results

**161 of 165 admitted cases pass. Four real failures were found.** Each one was judged against
independent source evidence, not against current engine output:

1. **`hep-python-06-repair` (upstream_gold).** The witnessed variable-misuse fix `max_depth -= 1` →
   `depth -= 1` is reported only as a REFACTORING rename of `max_depth` to `depth`. That presents a
   behavior-changing fix as a behavior-preserving refactor.
2. **`hep-js-06-repair` (upstream_gold).** The same fix in JavaScript is missed entirely. The only
   reported change is a spurious MOVE of the literal `1`, so the meaningful edit is lost.
3. **`hep-python-00-local-rename` (reviewed_external_derived).** Renaming the local `distance` to `gap`
   is reported as a DELETION and an ADDITION of the assignment statement, rather than as a rename.
4. **`hep-cpp-00-local-rename` (reviewed_external_derived).** Renaming the local loop variable `j` to
   `k` is reported as a DELETION and an ADDITION of the identifier in `int i,j;`.

These four are recorded as failures. No expectation was changed. None has a linked issue yet; each
needs its own bug. Two further results were checked:

- **Determinism.** A second identical run produced identical statuses and review summaries.
- **Regression gate.** With this report as `--baseline`, the gate exits 0.

### C/C++ `CPP/11` and the minimal character literal

Current core does **not** lose these with the locked C/C++ component
(`cpp_parser.wasm` `447196da…`, cpp-parser commit `d29063ac`):

- `hep-cpp-11-repair` passes, including both required `'1'`↔`'0'` literal facts.
- `literal-c-minimal-char` and `literal-cpp-minimal-char` pass. Each reports one MODIFICATION of
  `char_literal` `'0'` → `'1'` at bytes 16..19.

**No core fix is needed.** The fix lives in the parser component, cpp-parser #5.

The guard check is in `adversarial-old-cpp-component.json`. It replaced only `cpp_parser.wasm` with the
published 0.0.2b2 wheel bytes (`9af46c8b…`). With that component, all 9 literal regressions fail, and the
prefix case is even classified as style-only. So the oracle detects the loss, and the regression risk
remains for any consumer still shipping the old component.

## Not claimed

- Exact intent, correspondence and span accuracy are not certified.
- HumanEvalPack rows 12–163 are discovered but not imported.
- The parser-fidelity and robustness/scaling lanes have no admitted cases in any language.
- Translated HumanEvalPack tasks overlap across languages, so a portfolio total must deduplicate by
  task index.
- Tests are bounded evidence for the inputs they exercise; they are not program equivalence.

## #262 SQL and data-platform family

Ledger contribution: `scripts/external_families/family_262_sql_data.py` (merged by
`scripts/build_external_admission.py`). Witnesses: `scripts/data_family_witness.py`, with the ScriptDom tool in
`scripts/witness_tools/scriptdom/` and the M tool in `scripts/witness_tools/powerquery/`. The same core, C ABI
library (`ced9e0e1…`, built from `74618c31`; `crates/` is identical at `bbdec7c9`) and staged components
(manifest `23c390a8…`) were used as for the HumanEvalPack slice. The 2026-11-17 runner report is now the CI
baseline, and the gate exits 0 against it. Two full runs gave identical statuses, errors and review summaries.

### Admitted suites

Every suite below was pinned with `git ls-remote`. Selected blobs came from a blobless clone. The SHA-256 of
every retained file equals the bytes that `raw.githubusercontent.com/<repo>/<revision>/<path>` serves
(15 + 12 + 11 + 3 + 5 files).

| Suite (language) | Revision | Licence | Oracle executed |
|---|---|---|---|
| PostgreSQL `src/test/regress`, tag REL_16_15 (`sql`) | `7d3e000c5961a544302072058a1184e9a588837b` | PostgreSQL Licence (COPYRIGHT retained) | PostgreSQL 16.15 (Ubuntu 16.15-0ubuntu0.24.04.1); SQLite 3.53.1 via Python `sqlite3` |
| Microsoft SqlScriptDOM TestScripts + Baselines160 (`tsql`) | `3ea69b6fc1ed76b63eca6df868f3d1347b92e9fb` | MIT | NuGet ScriptDom 180.140.0 (18.0.140.0+9286938f) on .NET 8.0.34 |
| Microsoft powerquery-parser resources (`m`) | `84eb5f6f9f8f656dca69e687f514d7a2f86b5ef5` | MIT | npm @microsoft/powerquery-parser 4.0.1 on Node.js v22.23.4 |
| Azure Data Factory samples, V2 pipelines (`adf`) | `d470e0613b426db4e886cd57ccd6096e016464da` | MIT | strict JSON (Python 3.13.18) plus the documented dependency-condition model |
| utPLSQL examples (`plsql`), seed only | `77fc2b49bc288f5fd9523aa98ee76328adeb5769` | Apache-2.0 | none: no Oracle Database |

The ScriptDom and powerquery-parser binaries are the published release that matches the pinned source
(release notes / `package.json`). They are not builds of the pinned commit.

### Witnesses (44 records, 42 support their claim)

- **PostgreSQL files (7).** Each file was run the way pg_regress runs it: a fresh database, `psql -X -a -q`,
  and pg_regress's timezone, DateStyle and `lc_*` settings. All 7 transcripts equal the upstream
  `expected/*.out` byte for byte. Those 7 were the files that work standalone, out of 20 inspected; the other
  13 need `test_setup` and its `regress.so`, which this package does not ship.
  The statement inventory comes from the server's csvlog (`log_statement=all`) with SQLSTATE 42601 tracked,
  and each statement is located in the exact file bytes. `numeric` and `update` are inconclusive: their psql
  meta-commands send statements that are not in the file (8 and 53 respectively), and `update` also contains an
  intentional syntax error. Both stay candidates.
- **SQL pairs (12; 24 engine executions).** Each pair is the upstream fixture setup plus one upstream query,
  or a reviewed transformation of it. The result set of the final statement was compared on PostgreSQL 16.15
  and on SQLite 3.53.1: in order where the query defines a total order, otherwise as a bag. Both engines agree
  on every pair. The 6 behaviour changes differ on both engines. The 6 preservations are equal on both, and
  each one also rests on a stated SQL equivalence.
- **ScriptDom (10).**
  - All 5 scripts parse with 0 TSql160Parser errors.
  - TSql150Parser reproduces the upstream `Only160SyntaxTests` `nErrors150` for each (5, 5, 3, 11, 15).
  - For each of the 5 script/baseline pairs, Sql160ScriptGenerator renders identical scripts from both trees.
- **powerquery-parser (9).** All 9 resources reach an OK parse stage, which is what the upstream
  `lexParseResource` test asserts. Section members were inventoried from the upstream AST.
- **ADF (6).**
  - Both samples are strict JSON.
  - The key-reorder and minified controls are value-equal.
  - The dependency `Succeeded → Completed` edit changes eligibility after a failed predecessor (modelled, not
    run on the service).
  - The policy-timeout edit changes a value.

### Parser-fidelity adapter

When an admitted case's `expected.parse.statement_inventory` is set, the runner checks the parse tree
against the independent inventory (`external_ledger.judge_parse`):

- The top-level units are the direct children of the root.
- A node that spans more than one reference statement start is a grouping container (a transaction block,
  batch or section) and is replaced by its own children. No node type names are used.
- The number of units and each unit's start byte must equal the reference.

Without an inventory, the case only checks acceptance: no parse errors, no fallback and no invented changes.

### Cases (admitted / executed / passed) per language × class × lane

| Language | Class | Lane | Admitted | Executed | Passed | Notes |
|---|---|---|---:|---:|---:|---|
| sql | upstream_gold | parser_fidelity | 4 | 3 | 2 | plus 1 `unsupported` (explicit fallback); 2 candidates |
| sql | reviewed_external_derived | parser_fidelity | 1 | 1 | 1 | boolean SQL payload |
| sql | reviewed_external_derived | change_understanding | 12 | 12 | 9 | changed 6/6, preservation false alarms 3/6 |
| tsql | upstream_gold | parser_fidelity | 5 | 5 | 0 | |
| tsql | upstream_gold | change_understanding | 5 | 5 | 0 | all preservation pairs; no behaviour-change oracle |
| m | upstream_gold | parser_fidelity | 9 | 9 | 2 | |
| adf | upstream_gold | parser_fidelity | 2 | 2 | 2 | |
| adf | reviewed_external_derived | change_understanding | 4 | 4 | 4 | changed 2/2, preserved 2/2 |
| plsql | external_seed_only | parser_fidelity | 0 (4 candidates) | – | – | no Oracle |

### Real failures, minimised (recorded, not hidden; no issues filed)

Each failure is minimised to its smallest reproducing input.

**SQL (`sql_parser.wasm`)**

1. **Lost statements.** `CREATE DOMAIN d AS text; SELECT 1;` yields a tree that holds only the `SELECT`, and
   `CREATE OPERATOR` is lost the same way. Changing `AS text` to `AS int` is reported as style-only
   (`equivalent`). That is a missed meaningful change. In `case.sql`, 10 of the 64 reference statements are unmatched:
   2 CREATE DOMAIN and 2 CREATE OPERATOR have no node, and the 3 BEGIN and 3 ROLLBACK have no node of their
   own because they only bound a `transaction` container.
2. **Fuel exhaustion.** Five copies of `SELECT a FROM t HAVING a;` (130 bytes) trap with "all fuel consumed";
   four copies parse. HAVING without GROUP BY is the trigger: the same input with `GROUP BY a`, or with
   `WHERE`, is fine. The 50-line `select_having.sql` therefore falls back (`native_parse_incomplete`).
3. **Keyword case.** `SELECT a FROM t;` → `select a from t;` is reported as a semantic MODIFICATION of the
   `from` clause, whose label is the raw text.
4. **Comment.** `SELECT b FROM t GROUP BY b;` → `SELECT b FROM t /* g */ GROUP BY b;` is reported as a
   semantic MODIFICATION of the `from` clause.
5. **Alias rename.** In `SELECT x.b FROM t x, t y WHERE x.a = y.a;` → `p`/`q`, the unchanged column `a` is
   reported as a DELETION and an ADDITION.

**T-SQL (`tsql_parser.wasm`)**

6. **Empty trees.** Each of these parses to a tree with zero children:
   - a leading comment (`-- c` followed by `SELECT 1;`) or a UTF-8 BOM (`\xEF\xBB\xBFSELECT 1;`);
   - `UPDATE t SET a = 1;`, `DECLARE @a INT;` and `MERGE t USING s ON t.a = s.a WHEN MATCHED THEN DELETE;`.

   Each of the five upstream scripts starts with a comment, a BOM or an UPDATE, so all five show no statement
   nodes.
7. **Root range.** The root `source_file` range starts on line 1, not line 0. For the one-line `SELECT 1` it is
   `1:0-1:0`, and with no trailing newline it lies outside the source.
8. **Merged statements.** `SELECT 1;\nSELECT 2;` becomes one `select_statement` with two `select_clause`
   children, and GO batches are not separated.
9. **Keyword case.** `select a from t;` → `SELECT a FROM t;` is reported as a semantic MODIFICATION of
   `select_item` `a from t`. The FROM clause is absorbed into the select item, and this is why the
   Select/Window baseline pairs are classified as semantic.

**M (`dax_parser.wasm`)**

10. **Section members lost.** For a section document `section S; shared A = 1; B = 2;`, the tree has no member
    nodes, and editing `B = 2` → `B = 3` is reported as style-only. That is a missed meaningful change. The
    upstream resources keep at most the first `let` query; for example, HelloWorld.pq exposes 1 of 4 members.

### Blocked or left as a gap

- **SQLLogicTest (`sql`, the selected primary source): BLOCKED.** The egress proxy refuses sqlite.org and
  www.sqlite.org. No mirror was substituted.
- **ADF JSON schema: BLOCKED.** schema.management.azure.com is refused, so no schema validation is claimed.
- **plsql: GAP.** No Oracle Database is available, so the utPLSQL examples are retained as seed-only
  candidates. BIRD-CRITIC needs gated gold.
- **tsql behaviour changes: GAP.** There is no SQL Server and tSQLt needs one, so only preservation pairs
  exist and change recall is N/A.
- **m change lane: GAP.** No M evaluator is available. PQ/PQOut connector tests need PQTest on Windows, and
  the TripPin tutorial steps have no semantic oracle.
- **dax: GAP.** DAXBench access, licence and harness are unresolved, and no permissive DAX corpus with an
  oracle was found.
- **sas: GAP.** SASUnit is LGPL-3.0, which is not permissive, and needs a SAS runtime, so nothing was vendored.
- **databricks / databricks-workflow: BLOCKED by licence.** The CLI acceptance tests and bundle examples are
  under the restrictive Databricks License, which limits use to the Databricks Services, so nothing was
  vendored. The shared-component alias assertions remain open.
- **Robustness/scaling lane:** no admitted cases for any of the nine IDs. The fuel finding above was found
  by minimisation; it is not yet a scored robustness case.
