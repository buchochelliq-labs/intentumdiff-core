# External-benchmark scorecard: first slice, #262 and #265, 2026-11-17

This is the scorecard produced under the core #268 contract (schema-2 admission ledger). It covers three
contributions, all in one ledger and one runner report. **It does not certify any language.**

- **The first #261 slice:** HumanEvalPack tasks 0–11 in Python, JavaScript, Java, Go, C++ and Rust, plus
  handwritten C and C++ character-literal regressions.
- **#262, the SQL and data-platform family:** PostgreSQL regression (`sql`), Microsoft SqlScriptDOM
  (`tsql`), Microsoft powerquery-parser (`m`) and Azure Data Factory samples (`adf`), plus utPLSQL seeds
  (`plsql`). See the [#262 section](#262-sql-and-data-platform-family).
- **#265, the structured family:** `yaml`, `json`, `toml`, `ini`, `proto` and `graphql`; `xml` is BLOCKED
  in both lanes. See the [#265 section](#265-structured-family).

The ledger holds 2,282 cases (171 + 48 + 2,063), of which 2,133 are admitted and executed through the
C ABI (165 + 42 + 1,926). 58 of the 74 registered IDs have no measured external lane. No language has a
robustness/scaling result. The two family sections were first published as separate runs; this directory
now holds the single combined run, and the former `../2026-11-17-external-scorecard-265/` directory was
folded into it.

Files:

- `scorecard.md` and `scorecard.json`: rows for each language × suite × class × lane, with discovered,
  eligible, admitted, executed, passed, failed, unsupported, skipped and blocked counts. Every one of
  the 74 IDs has an entry.
- `results.json`: the runner report for all 2,133 admitted cases, with a compact review summary for each
  case. It is also the reviewed baseline for the CI regression gate.
- `parser_manifest.json`: the exact staged component identities.
- `adversarial-old-cpp-component.json`: the guard check described below.

## Combined run

- Core `786698a3` (`claude/magical-mendel-isxeqm`: release/v0.0.3-rc `5ec2105` plus the #262 and #265
  merges; `crates/` clean and identical to `74618c31`). Ledger SHA-256 `fd249308…`.
- The C ABI library is the same release build as every earlier run, SHA-256 `ced9e0e1…598b`, built from
  `74618c31` as described under Identity.
- Components: the same staged `build/benchmark-inputs` (manifest `23c390a8…`) that the first slice, #262 and
  #265 used. `parser_manifest.json` here is that manifest.
- Transport: intentumdiff-python `0dd7e50` on `PYTHONPATH`.
- **Determinism.** Two full runs gave identical statuses, errors, worker statuses, completeness and review
  summaries for all 2,133 cases.
- **Unchanged statuses.** Every case recorded by the three earlier runs (165 first slice, 207 with #262,
  2,091 with #265) has the same status, errors and review summary in this run. The gate exits 0 against
  each earlier report and against this one.
- **Scorecard change.** The parser-fidelity rows of #262 now score its reference-accepted documents as
  must-accept documents (valid-acceptance rate) instead of as preservation controls, the same as #265. The
  case statuses are unchanged.

### Results per language × lane (admitted external cases)

Rates are numerator/denominator over executed cases; `unsupported` cases are not executed. The full rows
per suite and class are in `scorecard.md`.

| Language | Lane | Executed | Passed | Failed | Unsupported | Valid accepted | Invalid rejected | Change recall | Preservation false alarms |
|---|---|---:|---:|---:|---:|---|---|---|---|
| adf | parser | 2 | 2 | 0 | 0 | 2/2 | N/A | N/A | N/A |
| adf | change | 4 | 4 | 0 | 0 | N/A | N/A | 2/2 | 0/2 |
| cpp | change | 32 | 31 | 1 | 0 | N/A | N/A | 17/17 | 1/15 |
| go | change | 21 | 21 | 0 | 0 | N/A | N/A | 9/9 | 0/12 |
| graphql | parser | 46 | 46 | 0 | 0 | 46/46 | N/A | N/A | N/A |
| graphql | change | 50 | 39 | 11 | 0 | N/A | N/A | 14/25 | 0/25 |
| ini | parser | 9 | 9 | 0 | 0 | 9/9 | N/A | N/A | N/A |
| ini | change | 13 | 13 | 0 | 0 | N/A | N/A | 6/6 | 0/7 |
| java | change | 27 | 27 | 0 | 0 | N/A | N/A | 12/12 | 0/15 |
| javascript | change | 27 | 26 | 1 | 0 | N/A | N/A | 11/12 | 0/15 |
| json | parser | 245 | 97 | 148 | 0 | 95/95 | 2/150 | N/A | N/A |
| json | change | 144 | 132 | 12 | 0 | N/A | N/A | 98/100 | 10/44 |
| m | parser | 9 | 2 | 7 | 0 | 2/9 | N/A | N/A | N/A |
| proto | parser | 185 | 173 | 12 | 0 | 173/185 | N/A | N/A | N/A |
| proto | change | 117 | 117 | 0 | 7 | N/A | N/A | 93/93 | 0/24 |
| python | change | 27 | 25 | 2 | 0 | N/A | N/A | 11/12 | 1/15 |
| rust | change | 27 | 27 | 0 | 0 | N/A | N/A | 12/12 | 0/15 |
| sql | parser | 4 | 3 | 1 | 1 | 3/4 | N/A | N/A | N/A |
| sql | change | 12 | 9 | 3 | 0 | N/A | N/A | 6/6 | 3/6 |
| toml | parser | 546 | 270 | 276 | 0 | 185/185 | 85/361 | N/A | N/A |
| toml | change | 77 | 72 | 5 | 0 | N/A | N/A | 48/48 | 5/29 |
| tsql | parser | 5 | 0 | 5 | 0 | 0/5 | N/A | N/A | N/A |
| tsql | change | 5 | 0 | 5 | 0 | N/A | N/A | N/A | 5/5 |
| yaml | parser | 303 | 270 | 33 | 0 | 229/229 | 41/74 | N/A | N/A |
| yaml | change | 184 | 52 | 132 | 0 | N/A | N/A | 6/6 | 132/178 |

The C handwritten regressions (4/4 passed) and the C++ ones (5/5) never enter a lane rate. Totals: 1,471
passed, 654 failed and 8 unsupported (handwritten 9/9; reviewed_external_derived 371 passed, 28 failed;
upstream_gold 1,091 passed, 626 failed, 8 unsupported).

### Baseline update: number respelling equivalence

The engine rule `keyed_data.number_exact_decimal_respelling` (JSON, ADF and Databricks `number` leaves
whose exact decimal is unchanged) turned four `reviewed_external_derived` JSON preservation false alarms
into passes: `jts-y_number_0e1-reformat`, `jts-y_number_double_close_to_zero-reformat`,
`jts-y_number_int_with_exp-reformat` and `jts-y_number_real_capital_e_neg_exp-reformat`. Each one is now
`completeness="equivalent"` with that rule as `metadata.equivalence_proof`. Only those four entries of
`results.json`, the scorecard regenerated from it, and the counts above changed; the identity fields
below still describe the original run. A full rerun with the new library changed no other status,
error, completeness or review summary, and no `upstream_gold` case. Still failing, and correct to
decline: `-0` → `0` (signed zero is not equated), and the spellings with `e+` that the pinned JSON
component splits (`1E22` → `1e+22` keeps only `22`, so the token cannot be proved).

## Identity (C ABI library and components)

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

## First-slice admitted cases (165 executed through the C ABI)

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

## Results (first slice)

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
- The robustness/scaling lane has no admitted cases in any language. (The first slice has no
  parser-fidelity cases; #262 and #265 add them.)
- Translated HumanEvalPack tasks overlap across languages, so a portfolio total must deduplicate by
  task index.
- Tests are bounded evidence for the inputs they exercise; they are not program equivalence.

## #262 SQL and data-platform family

Ledger contribution: `scripts/external_families/family_262_sql_data.py` (merged by
`scripts/build_external_admission.py`). Witnesses: `scripts/data_family_witness.py`, with the ScriptDom tool in
`scripts/witness_tools/scriptdom/` and the M tool in `scripts/witness_tools/powerquery/`. The same core, C ABI
library (`ced9e0e1…`, built from `74618c31`; `crates/` is identical at `bbdec7c9`) and staged components
(manifest `23c390a8…`) were used as for the HumanEvalPack slice. Its own two runs gave identical statuses,
errors and review summaries, and the combined run above reproduces them.

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
#262 cases carry no explicit `accepts` claim, so a fallback on a reference-accepted document is
`unsupported`, not failed. This is one adapter shared with #265's must-accept/must-reject claims; see
`docs/EXTERNAL_BENCHMARKS.md`.

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

## #265 structured family

This family adds the core #265 data/schema IDs to the #268 ledger: `yaml`, `json`, `toml`, `xml`, `ini`,
`proto` and `graphql`. Its fragment is `tests/fixtures/external/admission/families/structured-265.json`,
written and `--check`-ed by `scripts/structured_family.py` and merged by
`scripts/build_external_admission.py`. `xml` is BLOCKED in both lanes. No language has a
robustness/scaling result.

### Identity

- Core `bbdec7c9` (crates identical to `74618c31`, clean). The C ABI library is the same release build
  as the first slice, SHA-256 `ced9e0e1…598b`.
- Components: the same staged `build/benchmark-inputs` as the first slice (manifest `23c390a8…`). The
  family's components are `yaml_parser.wasm` `2ff91eb6…`, `json_parser.wasm` `ffb886cf…`,
  `toml_parser.wasm` `af1ec51b…`, `ini_parser.wasm` `dc665e89…`, `proto_parser.wasm` `d692e7aa…`,
  `graphql_parser.wasm` `b01f0a4b…` and `xml_parser.wasm` `2bd8f26b…` (not exercised).
- Transport: intentumdiff-python `0dd7e50`.
- Determinism: its own second run gave identical statuses, errors and review summaries for all 2,091
  cases, and the combined run above reproduces them.

### Sources (all pinned to a commit; bytes and SHA-256 are in `upstream/structured/<suite>/ACQUISITION.json`)

| Suite | Revision | Licence | Enumeration |
|---|---|---|---|
| YAML Test Suite, `data` branch | `6ad3d2c62885d82fc349026c136ef560838fdf3d` | MIT (`License` at main `da267a5c`) | **incomplete**: no in-suite index. 333 candidate IDs were probed and 385 (sub)tests found. |
| JSONTestSuite `test_parsing` | `1ef36fa01286573e846ac449e8683f8833c5b26a` | MIT | names in `results/parsing.html` (lower bound; the index is hashed, not vendored) |
| JSON Patch Tests | `2a928f9044aad35c74e2788d498bcf2c6b91adea` | Apache-2.0 (README) | `tests.json`, `spec_tests.json` |
| toml-test v1.6.0 | `369cc3ae275fd76b6e21780ebf87609e5b2a951a` | MIT (`COPYING`) | `tests/files-toml-1.0.0` (complete) |
| CPython v3.13.18 configparser | `0d4bd512fc429e3d4eacdd113c975e161f21644d` | PSF-2.0 | `configdata/cfgparser.{1,2,3}` |
| inih | `2bbdec4a366c8c39746ee0982e7ca0febbb044b6` | BSD-3-Clause | `.ini` names in `tests/unittest.c` |
| buf v1.75.0 breaking fixtures | `1a3a05a3e0fa350c4b814b7f48aa0e64e9a906bd` | Apache-2.0 | dirs from `breaking_test.go`; file names probed (`1..9.proto`, `a/b.proto`, `buf.yaml`) |
| graphql-js v16.15.0 | `2b4f68feaed36fd3b221764a467c6fa78a0c9797` | MIT | `findBreakingChanges-test.ts` |

Tags and branches were resolved to commits with the Go module proxy's VCS origin record. Bytes were then
fetched only from `raw.githubusercontent.com/<repo>/<commit>/<path>`.

**Blocked acquisitions:**

- github.com HTML, the REST API and git clones are unreachable for these repositories, so no tree listing
  was possible. YAML and buf enumeration is therefore partial. Two buf cases (`breaking_ignore_unstable_*`)
  need files under subdirectories and stay candidates.
- **W3C XML Conformance Test Suite:** `www.w3.org` refuses the CONNECT tunnel (403). It is recorded in the
  ledger's `blocked_acquisitions`, and `xml` is BLOCKED in both lanes. No other source was substituted.

### Oracles (2,063 executions, `scripts/structured_family.py witness`)

| Oracle | Reference | supports / other |
|---|---|---|
| JSON parse, typed equality | CPython 3.13.18 `json` (strict; NaN/Infinity rejected) | 316 / 0 |
| JSON Patch | `jsonpatch` 1.33 must reproduce the upstream `expected`, then typed equality | 73 / 1 inconclusive (library fails on root `add`) |
| TOML parse, typed equality | `tomllib`; valid files must also equal toml-test's tagged JSON | 623 / 0 |
| YAML parse, typed equality | ruamel.yaml 0.18.16 safe loader (YAML 1.2), compared with `in.json` | 487 / 134 (68 contradict, 66 inconclusive) |
| INI | configparser under each upstream test's options and assertions; inih's own `unittest.c` built with gcc 13.3 and compared byte-for-byte with `baseline_multi.txt` | 22 / 0 |
| proto | buf 1.75.0 `build`; `breaking` must reproduce the exact upstream annotation set; image equality without source info | 309 / 2 inconclusive |
| graphql | graphql-js 16.15.0 `buildSchema`; `findBreaking/DangerousChanges` must equal the upstream assertions; sorted `printSchema` equality | 96 / 0 |

Only `supports_claim` admits a case. Where the YAML reference disagrees with the suite label, the case
stays a candidate. That biases the admitted YAML subset toward inputs ruamel handles.

### Results (admitted #265 cases through the C ABI)

| Language | Lane | Class | Passed / executed | Notes |
|---|---|---|---|---|
| json | parser | upstream_gold | 97/245 | valid accepted 95/95; **invalid rejected 2/150** |
| json | change | reviewed_external_derived | 132/144 | change recall 98/100; preservation false alarms 10/44 (was 14/44; see "Baseline update") |
| toml | parser | upstream_gold | 270/546 | valid 185/185; **invalid rejected 85/361** |
| toml | change | reviewed_external_derived | 72/77 | recall 48/48; preservation failures 5/29 (all `incomplete`) |
| yaml | parser | upstream_gold | 270/303 | valid 229/229; invalid rejected 41/74 |
| yaml | change | upstream_gold (in→out) | 46/172 | preservation false alarms 126/172 |
| yaml | change | reviewed_external_derived | 6/12 | recall 6/6; comment false alarms 6/6 |
| ini | parser | upstream_gold | 9/9 | accept-only (no dialect-neutral reject rule) |
| ini | change | reviewed_external_derived | 13/13 | |
| proto | parser | upstream_gold | 173/185 | 12 valid files rejected by explicit fallback |
| proto | change | upstream_gold | 93/93 executed (+7 unsupported) | buf pairs |
| proto | change | reviewed_external_derived | 24/24 | |
| graphql | parser | upstream_gold | 46/46 | |
| graphql | change | upstream_gold | 14/25 | **11 schema changes reported as style-only** |
| graphql | change | reviewed_external_derived | 25/25 | |

### Real failures, minimised (not filed)

Each reproducer was rechecked directly through `live_diff_contents`/`parse_to_tree`:

1. **GraphQL field types are not represented.** `type Query {\n  foo: Float\n}` → `foo: String` gives
   `has_semantic_changes=false, is_style_only=true`. The tree has `field 'foo'` with no type, so nullability
   and type changes are invisible.
2. **GraphQL single-line type bodies lose their fields.** In `type Query { foo: Float }`, the tree is
   `type_definition 'Query'` with no children, so even a field rename is style-only.
3. **JSON numbers with an explicit `+` exponent are truncated.** `[1e+22]` parses as `number '22'` and
   `[1e+2]` as `number '1'`. Both `[1e+22]`→`[2e+22]` and `[1e+2]`→`[1e+3]` are style-only.
4. **JSON/TOML/YAML accept invalid input silently.** `[1 true]` is parsed as `array[true]` with no
   diagnostics: the `1` is dropped, and `[1 true]`→`[2 true]` is style-only. TOML `a = [1,,2]` and YAML
   2CMS (`this\n is\n  invalid: x`) give no `parse_errors` and no fallback. 148 of 150 JSON, 276 of 361
   TOML and 33 of 74 YAML must-reject documents are affected.
5. **YAML double-quoted scalars with an escape lose their text.** `"a\/b"` is labelled `\/`, and
   `k: "a\nb"` → `k: "c\nd"` is style-only (a value change missed).
6. **YAML comments become node labels.** `a: 1` → `# c\na: 1` reports `MODIFICATION stream 'stream' → '# c'`,
   and a trailing comment reports `block_mapping → '# c'`. Quoting style is compared lexically:
   `k: a` → `k: 'a'` is a MODIFICATION. Together these cause most of the 126 in→out false alarms; 28 more
   are `incomplete` reviews.
7. **proto fallback on valid files (buf build succeeds).** Two inputs trigger it:
   `syntax = "proto2";\nmessage A {\n  optional group Foo = 1 {\n  }\n}` and the edition-2023 option
   `string s = 1 [features.(pb.cpp).string_type = VIEW];`.
8. **JSON re-serialisation false alarms.** Number respelling (`1E22`→`1e+22`) and escape
   normalisation are reported as semantic MODIFICATIONs, even though the typed values are equal.
9. **TOML `8080` → `8_080`** returns `completeness: incomplete` and is not proven equivalent.

### Not claimed

- XML is not covered: its acquisition is blocked. No lane has robustness/scaling cases.
- INI must-reject behaviour is not admitted, because the route declares no dialect.
- YAML enumeration is partial, and the admitted YAML subset is filtered by agreement with ruamel.
- Compatibility labels (buf, graphql-js) are one semantic dimension, not author intent.
- Derived edits are bounded at 24 per derivation and suite, and are built deterministically from sorted seeds.
