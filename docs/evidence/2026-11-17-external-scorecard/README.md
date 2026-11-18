# External-benchmark scorecard: first slice, #262, #265, #263 and #267, 2026-11-17

This is the scorecard produced under the core #268 contract (schema-2 admission ledger). It covers five
contributions, all in one ledger and one runner report. **It does not certify any language.**

- **The first #261 slice:** HumanEvalPack tasks 0–11 in Python, JavaScript, Java, Go, C++ and Rust, plus
  handwritten C and C++ character-literal regressions.
- **#262, the SQL and data-platform family:** PostgreSQL regression (`sql`), Microsoft SqlScriptDOM
  (`tsql`), Microsoft powerquery-parser (`m`) and Azure Data Factory samples (`adf`), plus utPLSQL seeds
  (`plsql`). See the [#262 section](#262-sql-and-data-platform-family).
- **#265, the structured family:** `yaml`, `json`, `toml`, `ini`, `proto` and `graphql`; `xml` is BLOCKED
  in both lanes. See the [#265 section](#265-structured-family).
- **#263, the web, markup and embedded family:** `markdown`, `mdx`, `html`, `css`, `scss`, `vue`,
  `svelte`, `astro`, `latex`, `asciidoc` and `po`; `postscript` and `generic` are BLOCKED. See the
  [#263 section](#263-web-markup-and-embedded-family).
- **#267, the build, configuration and infrastructure family:** `dockerfile`, `hcl`, `gomod`, `gitignore`,
  `cmake`, `make` and `puppet`. See the [#267 section](#267-build-configuration-and-infrastructure-family).

The ledger holds 5,555 cases (171 + 48 + 2,063 + 2,497 + 776), of which 5,054 are admitted and executed
through the C ABI (165 + 42 + 1,926 + 2,160 + 761). 34 of the 74 registered IDs have a measured external
lane; 40 have none. No language has a robustness/scaling result. The family sections were first published
as separate runs; this directory now holds the single combined run. The former
`../2026-11-17-external-scorecard-265/` and `../2026-11-17-external-scorecard-267/` directories were folded
into it.

Files:

- `scorecard.md` and `scorecard.json`: rows for each language × suite × class × lane, with discovered,
  eligible, admitted, executed, passed, failed, unsupported, skipped and blocked counts. Every one of
  the 74 IDs has an entry.
- `results.json`: the runner report for all 5,054 admitted cases, with a compact review summary for each
  case. It is also the reviewed baseline for the CI regression gate.
- `parser_manifest.json`: the exact staged component identities.
- `adversarial-old-cpp-component.json`: the guard check described below.

## Combined run (registry #21 components)

- Core `1adcadea` (`claude/magical-mendel-isxeqm`: release/v0.0.3-rc `1c4bdbc` plus the #263 and #267
  merges; `crates/` clean and identical to `1c4bdbc`). Ledger SHA-256 `b9dd595f…51f0`.
- C ABI library: `cargo +1.95.0 build --locked --release --lib` at `1c4bdbc` with
  `CARGO_PROFILE_RELEASE_DEBUG=0`, SHA-256 `4ebfe9d3…658d`.
- Components: staged as CI does, with `scripts/stage_external_benchmark_inputs.py --download-artifacts` from
  registry `eb02686b` (#21), the published `intentumdiff-python==0.0.2b2` wheel as byte mirror and the 43
  locked CI artifacts in `scripts/external_benchmark_inputs.lock.json`. Manifest `6af77de6…7441`. Against
  the registry #20 staging of the previous baseline (`70944de9…`) the only changed component is
  `make_parser.wasm` (`684c554c…` → `32c73eea…`, make-parser#9); no earlier case routes to it.
- Transport: the published `intentumdiff-python==0.0.2b2` wheel, as in CI.
- **Determinism.** Two full runs gave identical statuses, errors, worker statuses, completeness and review
  summaries for all 5,054 cases (only timings differ).
- **Unchanged statuses.** All 2,133 cases of the previous baseline (first slice, #262, #265) have the same
  status, errors, completeness and review summary. The gate exits 0 against the previous `results.json`
  and against this one.
- **#263 and #267 on the current components.** Both families were first measured on the older
  manifest `23c390a8…` (registry `28f5fce5`, library `ced9e0e1…`). On the current components:
  - #263: 2,003 passed and 157 failed (was 1,964 / 196). 39 cases flip from failed to passed and none
    regresses: `astro` text edits 17, `svelte` legacy text edits and a script-quote pair 10, `vue` script
    quote and text edits 9, `asciidoc` 3 (`adoc-inline-callout-basic` parser, its `word_edit` pair and the
    `image-with-link` link-target pair). 13 passing Svelte migrate pairs changed only their review summary.
  - #267: 624 passed, 126 failed and 11 unsupported (was 622 / 128 / 11). Three GNU Make flavor pairs
    (`gmake-variables-flavors-00-changed-cbace1`, `-01-changed-6abfba`, `-02-changed-20a712`) now pass,
    and one regresses: `gmake-variables-flavors-01-equivalent-d51f29` (`:=` → `::=`, which GNU Make treats
    identically) is now a MODIFICATION of `assignment_operator`. It is `reviewed_external_derived`, so
    the gate does not fail on it. Two Dockerfile must-accept files with an unterminated JSON array
    (`bk-jeztah-invalid-json-unterminated-bracket-parse`, `-string-parse`) still fail, now with a
    `native_parse_incomplete` fallback instead of an empty tree. Two passing cases changed only their
    review summary (`bk-mail-cmd-exec-form`, `gmake-variables-flavors-00-changed-8b9724`).

### Results per language × lane (admitted external cases)

Rates are numerator/denominator over executed cases; `unsupported` cases are not executed. Handwritten
regressions are excluded. The full rows per suite and class are in `scorecard.md`.

| Language | Lane | Executed | Passed | Failed | Unsupported | Valid accepted | Invalid rejected | Change recall | Preservation false alarms |
|---|---|---:|---:|---:|---:|---|---|---|---|
| adf | parser | 2 | 2 | 0 | 0 | 2/2 | N/A | N/A | N/A |
| adf | change | 4 | 4 | 0 | 0 | N/A | N/A | 2/2 | 0/2 |
| asciidoc | parser | 68 | 68 | 0 | 0 | 68/68 | N/A | N/A | N/A |
| asciidoc | change | 41 | 41 | 0 | 0 | N/A | N/A | 19/19 | 0/22 |
| astro | parser | 238 | 238 | 0 | 0 | 238/238 | N/A | N/A | N/A |
| astro | change | 60 | 58 | 2 | 0 | N/A | N/A | 44/46 | 0/14 |
| cmake | parser | 108 | 71 | 37 | 0 | 65/88 | 6/20 | N/A | N/A |
| cmake | change | 9 | 6 | 3 | 1 | N/A | N/A | 2/4 | 1/5 |
| cpp | change | 27 | 26 | 1 | 0 | N/A | N/A | 12/12 | 1/15 |
| css | parser | 24 | 24 | 0 | 0 | 24/24 | N/A | N/A | N/A |
| css | change | 6 | 6 | 0 | 0 | N/A | N/A | N/A | 0/6 |
| dockerfile | parser | 36 | 18 | 18 | 0 | 16/34 | 2/2 | N/A | N/A |
| dockerfile | change | 6 | 6 | 0 | 9 | N/A | N/A | 3/3 | 0/3 |
| gitignore | parser | 7 | 7 | 0 | 0 | 7/7 | N/A | N/A | N/A |
| gitignore | change | 12 | 7 | 5 | 0 | N/A | N/A | 6/6 | 5/6 |
| go | change | 21 | 21 | 0 | 0 | N/A | N/A | 9/9 | 0/12 |
| gomod | parser | 139 | 137 | 2 | 0 | 137/139 | N/A | N/A | N/A |
| gomod | change | 16 | 10 | 6 | 1 | N/A | N/A | 6/7 | 5/9 |
| graphql | parser | 46 | 46 | 0 | 0 | 46/46 | N/A | N/A | N/A |
| graphql | change | 50 | 50 | 0 | 0 | N/A | N/A | 25/25 | 0/25 |
| hcl | parser | 62 | 55 | 7 | 0 | 53/55 | 2/7 | N/A | N/A |
| hcl | change | 19 | 16 | 3 | 0 | N/A | N/A | 9/9 | 3/10 |
| html | parser | 164 | 164 | 0 | 0 | 164/164 | N/A | N/A | N/A |
| html | change | 125 | 75 | 50 | 0 | N/A | N/A | 21/47 | 24/78 |
| ini | parser | 9 | 9 | 0 | 0 | 9/9 | N/A | N/A | N/A |
| ini | change | 13 | 13 | 0 | 0 | N/A | N/A | 6/6 | 0/7 |
| java | change | 27 | 27 | 0 | 0 | N/A | N/A | 12/12 | 0/15 |
| javascript | change | 27 | 26 | 1 | 0 | N/A | N/A | 11/12 | 0/15 |
| json | parser | 245 | 97 | 148 | 0 | 95/95 | 2/150 | N/A | N/A |
| json | change | 144 | 132 | 12 | 0 | N/A | N/A | 98/100 | 10/44 |
| latex | parser | 9 | 9 | 0 | 0 | 9/9 | N/A | N/A | N/A |
| latex | change | 12 | 6 | 6 | 0 | N/A | N/A | 3/6 | 3/6 |
| m | parser | 9 | 2 | 7 | 0 | 2/9 | N/A | N/A | N/A |
| make | parser | 70 | 64 | 6 | 0 | 64/65 | 0/5 | N/A | N/A |
| make | change | 8 | 7 | 1 | 0 | N/A | N/A | 4/4 | 1/4 |
| markdown | parser | 674 | 662 | 12 | 0 | 662/674 | N/A | N/A | N/A |
| markdown | change | 106 | 86 | 20 | 0 | N/A | N/A | 50/58 | 12/48 |
| mdx | parser | 30 | 30 | 0 | 0 | 30/30 | N/A | N/A | N/A |
| mdx | change | 48 | 42 | 6 | 0 | N/A | N/A | 42/42 | 6/6 |
| plsql | parser | 0 | 0 | 0 | 0 | N/A | N/A | N/A | N/A |
| po | parser | 19 | 18 | 1 | 0 | 18/18 | 0/1 | N/A | N/A |
| po | change | 33 | 20 | 13 | 0 | N/A | N/A | 9/9 | 13/24 |
| proto | parser | 185 | 173 | 12 | 0 | 173/185 | N/A | N/A | N/A |
| proto | change | 117 | 117 | 0 | 7 | N/A | N/A | 93/93 | 0/24 |
| puppet | parser | 251 | 213 | 38 | 0 | 212/234 | 1/17 | N/A | N/A |
| puppet | change | 7 | 7 | 0 | 0 | N/A | N/A | 3/3 | 0/4 |
| python | change | 27 | 25 | 2 | 0 | N/A | N/A | 11/12 | 1/15 |
| rust | change | 27 | 27 | 0 | 0 | N/A | N/A | 12/12 | 0/15 |
| scss | parser | 186 | 170 | 16 | 0 | 170/173 | 0/13 | N/A | N/A |
| scss | change | 84 | 58 | 26 | 0 | N/A | N/A | 34/37 | 23/47 |
| sql | parser | 4 | 3 | 1 | 1 | 3/4 | N/A | N/A | N/A |
| sql | change | 12 | 9 | 3 | 0 | N/A | N/A | 6/6 | 3/6 |
| svelte | parser | 90 | 86 | 4 | 0 | 86/86 | 0/4 | N/A | N/A |
| svelte | change | 44 | 43 | 1 | 0 | N/A | N/A | 9/9 | 1/35 |
| toml | parser | 546 | 270 | 276 | 0 | 185/185 | 85/361 | N/A | N/A |
| toml | change | 77 | 72 | 5 | 0 | N/A | N/A | 48/48 | 5/29 |
| tsql | parser | 5 | 1 | 4 | 0 | 1/5 | N/A | N/A | N/A |
| tsql | change | 5 | 0 | 5 | 0 | N/A | N/A | N/A | 5/5 |
| vue | parser | 88 | 88 | 0 | 0 | 88/88 | N/A | N/A | N/A |
| vue | change | 11 | 11 | 0 | 0 | N/A | N/A | 2/2 | 0/9 |
| yaml | parser | 303 | 270 | 33 | 0 | 229/229 | 41/74 | N/A | N/A |
| yaml | change | 184 | 71 | 113 | 0 | N/A | N/A | 6/6 | 113/178 |

The C handwritten regressions (4/4 passed) and the C++ ones (5/5) never enter a lane rate. Totals: 4,129
passed, 906 failed and 19 unsupported (handwritten 9/9; reviewed_external_derived 898 passed, 159 failed,
10 unsupported; upstream_gold 3,222 passed, 747 failed, 9 unsupported).

The two baseline updates below were applied to the previous baseline before #263 and #267 were folded in;
their counts are those of the 2,133 earlier cases, which this run reproduces exactly.

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

### Baseline update (registry #20 components)

`scripts/external_benchmark_inputs.lock.json` now follows registry `9889613d` (the #20 merge) instead of
`28f5fce5`. Registry #20 re-pinned 41 parser components to their CI-built 0.0.3 RC heads. The
`intentumdiff-python==0.0.2b2` wheel mirrors none of the new pins, so the lock now holds the exact CI
artifact for each of the 40 re-pinned components in the inventory (43 entries with the unchanged JSON, SQL
and ADF pins); the other 25 components still come from the wheel. 38 staged component hashes changed
(Databricks was re-pinned to a new commit with identical bytes). `results.json`, `parser_manifest.json` and
the scorecard are now one full rerun on these components:

- Core `58b462e5` (crates clean), C ABI library `cargo +1.95.0 build --locked --release --lib` with
  `CARGO_PROFILE_RELEASE_DEBUG=0`, SHA-256 `5f664627…fd40`; manifest `70944de9…4d4f`; transport from the
  published `intentumdiff-python==0.0.2b2` wheel, as in CI.
- **31 cases flip from failed to passed, and none regresses.** No `upstream_gold` case that passed before
  fails now; the gate exits 0 against the previous `results.json`.
  - `graphql` `upstream_gold` change, 11: `gql-01`, `-02`, `-06`, `-09`, `-13`, `-14`, `-21`, `-22`,
    `-27`, `-28`, `-29` (graphql `b01f0a4b…` → `97402e30…`).
  - `tsql` `upstream_gold` parser, 1: `scriptdom-SelectStatementTests160-statements` (tsql `70434085…` →
    `2904e246…`).
  - `yaml`, 19 (yaml `2ff91eb6…` → `f97b0930…`): `upstream_gold` change 13 (`yts-2XXW-out`, `-5WE3-out`,
    `-6JQW-out`, `-7BMT-out`, `-7BUB-out`, `-F8F9-out`, `-J7PZ-out`, `-JHB9-out`, `-L383-out`, `-RTP8-out`,
    `-S4JQ-out`, `-SYW4-out`, `-U3XV-out`) and `reviewed_external_derived` comment pairs 6
    (`yts-9KAX-comment`, `-AZ63-comment`, `-H2RW-comment`, `-J7VC-comment`, `-KMK3-comment`,
    `-ZWK4-comment`).
- The four JSON number-respelling passes recorded in the previous update are still passes.
- Without changing status, 23 still-failing cases changed their errors, completeness or review summary
  (`tsql` 9, `m` 7, `yaml` 7), and 9 passing cases changed their review summary (`graphql` 8, `rust` 1).
- The family sections below (#262, #265, the minimised failures and the original Identity) describe the
  earlier component set, except for the rows marked "see Baseline update (registry #20 components)".

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
| tsql | upstream_gold | parser_fidelity | 5 | 5 | 1 | was 0; see "Baseline update (registry #20 components)" |
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
| yaml | change | upstream_gold (in→out) | 59/172 | preservation false alarms 113/172 (was 46/172 and 126/172; see "Baseline update (registry #20 components)") |
| yaml | change | reviewed_external_derived | 12/12 | recall 6/6; comment false alarms 0/6 (was 6/12 and 6/6; see "Baseline update (registry #20 components)") |
| ini | parser | upstream_gold | 9/9 | accept-only (no dialect-neutral reject rule) |
| ini | change | reviewed_external_derived | 13/13 | |
| proto | parser | upstream_gold | 173/185 | 12 valid files rejected by explicit fallback |
| proto | change | upstream_gold | 93/93 executed (+7 unsupported) | buf pairs |
| proto | change | reviewed_external_derived | 24/24 | |
| graphql | parser | upstream_gold | 46/46 | |
| graphql | change | upstream_gold | 25/25 | was 14/25 with **11 schema changes reported as style-only**; see "Baseline update (registry #20 components)" |
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

## #263 web, markup and embedded family

Fragment: `tests/fixtures/external/admission/families/markup-263.json`, written and `--check`-ed by
`scripts/markup_family.py` and merged by `scripts/build_external_admission.py`. Bytes:
`scripts/acquire_markup_benchmarks.py`. Run on core `1d835646` with the same C ABI library (`ced9e0e1…598b`)
and staged components (manifest `23c390a8…`) as every earlier run; intentumdiff-python `0dd7e50`.
Two full runs gave identical statuses, errors and review summaries for all 4,293 cases, every one of the
2,133 earlier cases kept its status, errors and review summary, and the gate exits 0 against the earlier
and the new `results.json`. `tsx` (also in #263's scope) was not part of this task and stays a GAP.

That first run gave 1,964 passed and 196 failed. The results, table and failures below are from the
combined run on the registry #21 components (see [Combined run](#combined-run-registry-21-components)):
2,003 passed and 157 failed.

### Sources

Revisions were resolved with `git ls-remote`; bytes come from `raw.githubusercontent.com` at the commit, or
from the npm tarball whose `gitHead` is the commit. Tree listings are unreachable, so most enumerations are
probes of recalled names (recorded as incomplete).

| Suite (language) | Revision | Licence | Reference executed |
|---|---|---|---|
| CommonMark spec 0.31.2 (`markdown`), npm `commonmark-spec` (tarball `588280df…`, byte-equal to raw) | `9103e341a973013013bb1a80e13567007c5cef6f` | CC-BY-SA-4.0 | commonmark.js 0.31.2 |
| GFM spec, cmark-gfm 0.29.0.gfm.13 (`markdown`, extension examples only) | `587a12bb54d95ac37241377e6ddc93ea0e45439b` | CC-BY-SA-4.0 | cmark-gfm via cmarkgfm 2025.10.22 |
| WPT `html/syntax/parsing/resources/*.dat` (`html`; html5lib-tests says the tree tests now live here) | `edd042b63671f8ba064d86560836171e0cdb3956` | BSD-3-Clause (tests from html5lib, MIT) | html5lib 1.1 |
| css-parsing-tests (`css`) | `203ce36bffd617db7f118c551e32794561fb273d` | CC0-1.0 | tinycss2 1.5.1 |
| sass-spec, npm `sass-spec` 3.5.4-1 (tarball `890c0af3…`; basic, scss, css, errors dirs) | `8be4e1b7555e5560ed1767605ebefc0429756ece` | MIT | Dart Sass 1.108.2 |
| Svelte parser-modern, parser-legacy and migrate samples (`svelte`) | `23bb146306ff02cc5041bca0c5de9f8bd086e4a3` (svelte@5.57.3) | MIT | svelte 5.57.3 compiler + `svelte/server` |
| Vue compiler-sfc `parse.spec.ts`, `compileScript.spec.ts` (`vue`) | `8b479a89445868d474f17e2d837765e7c9ece5e4` (v3.5.45) | MIT | @vue/compiler-sfc + vue/server-renderer 3.5.45 |
| Astro `internal/printer/printer_test.go` (`astro`) | `271eee2e499432015ab4c6dc510c5331e1961b84` (@astrojs/compiler@4.0.0) | MIT | @astrojs/compiler 4.0.0 |
| MDX `docs/**/*.mdx` (`mdx`; project sample corpus, not a test suite) | `50aa8df0b027c893dec9f97a2b7c51539e9f1a4b` (3.1.1) | MIT | @mdx-js/mdx 3.1.1 |
| Asciidoctor DocTest `data/examples/asciidoc` (`asciidoc`; only the inline_*, document, embedded files exist) | `1f48884cb953e0315490837fbc30c1558c5752a0` | MIT | Asciidoctor 2.0.26 |
| polib test fixtures (`po`) | `bcb2bbab34448ca2e703a364ee523a6241eeff72` | MIT | GNU msgfmt 0.21 `--check` |
| LaTeX2e `base/manifest.txt` documents (`latex`) | `bd4c0b10fb93bd0dfdf2ed1b4660b05ff74fd2ca` | LPPL-1.3c | pdfTeX 1.40.25 (TeX Live 2023, texlive-latex-base) |

Blocked acquisitions: **postscript**, Ghostscript examples are AGPL-3.0 and no permissive suite with an
oracle was found; **generic**, WikiAtomicEdits data is only on storage.googleapis.com, which answers 403.

### Admitted cases and results (C ABI)

Parser-fidelity cases are must-accept unless a must-reject count is shown. Change pairs are
`reviewed_external_derived` except the 17 Svelte migrate pairs (`upstream_gold`).

| Language | Parser passed / executed | Valid accepted | Invalid rejected | Change passed / executed | Change recall | Preservation false alarms |
|---|---|---|---|---|---|---|
| markdown | 662/674 | 662/674 | N/A | 86/106 | 50/58 | 12/48 |
| mdx | 30/30 | 30/30 | N/A | 42/48 | 42/42 | 6/6 |
| html | 164/164 | 164/164 | N/A | 75/125 | 21/47 | 24/78 |
| css | 24/24 | 24/24 | N/A | 6/6 | N/A | 0/6 |
| scss | 170/186 | 170/173 | 0/13 | 58/84 | 34/37 | 23/47 |
| svelte | 86/90 | 86/86 | 0/4 | 43/44 | 9/9 | 1/35 |
| vue | 88/88 | 88/88 | N/A | 11/11 | 2/2 | 0/9 |
| astro | 238/238 | 238/238 | N/A | 58/60 | 44/46 | 0/14 |
| latex | 9/9 | 9/9 | N/A | 6/12 | 3/6 | 3/6 |
| asciidoc | 68/68 | 68/68 | N/A | 41/41 | 19/19 | 0/22 |
| po | 18/19 | 18/18 | 0/1 | 20/33 | 9/9 | 13/24 |

486 cases stay candidates: the reference disagreed with the claim (e.g. `*x*`→`_x_` in delimiter-rule
examples renders differently; Astro keeps frontmatter verbatim), or could not run (51 LaTeX documents need
packages outside texlive-latex-base, Svelte migrate samples that import sibling components, sass-spec
expectations from 2018 that Dart Sass 1.108 no longer reproduces).

### Real failures, minimised (not filed)

1. **Markdown block loss.** `---\n---\n` gives a tree with no nodes, and `---\nFoo\n---\nBar\n` loses the
   thematic break and the `Foo` setext heading. Six more CommonMark examples split or merge container blocks
   differently from the reference (lazy continuation `>     foo\n    bar`, list continuation `- one\n\n two`,
   `10) foo\n   - bar`); three report a section wrapper that starts on a leading blank line.
2. **Markdown presentation reported as semantic.** `*foo*`→`_foo_` is a paragraph MODIFICATION; an
   appended blank line after a list item is an ADDITION of `block_continuation`; editing the text of a
   setext heading is reported only as a REFACTORING of the section, not as a text edit.
3. **HTML text in implied-end elements is invisible.** `<p>foo<p>bar` → `<p>foox<p>bar` is style-only;
   `<p>foo</p>`→`<p>foox</p>` is correct. Upper-casing a tag name (`<div>`→`<DIV>`) is semantic, and
   swapping two attributes returns `completeness: incomplete`.
4. **Astro and Svelte element text** (`<button>Click</button>` → `Clickx` was style-only in both) is fixed by the
   registry #21 components. Still style-only: CSS text inside `<noscript><style>` in Astro
   (`div { color: red; }` → `divx { … }`, printer cases 044 and 045).
5. **Quote respelling in script imports** (`'svelte'` → `"svelte"`, also in Vue `<script setup>`) is no longer
   reported as a MODIFICATION on the registry #21 components.
6. **SCSS.** A leading `// note` line relabels the root (`stylesheet` → `// derived note`), so every
   silent-comment control is a false alarm; a number inside `@include` arguments is missed; three valid
   files (`$a: null`, IE `filter: alpha(opacity=…)`, hyphen/underscore names) fall back with "Empty semantic
   tree"; 13 syntax errors (e.g. `--prop: );`, `@supports {}`) are accepted without diagnostics.
7. **Svelte loose-mode inputs** (`{#if }`, `<div foo={a.}>`, unclosed tags) are accepted without diagnostics.
8. **PO.** A translator comment is an ADDITION reported as semantic; `test_syntax_error1.po` (a second
   `msgstr` without `msgid`) is accepted without diagnostics.
9. **LaTeX.** `Hello world` → `Hello  world` is a text MODIFICATION; `\section{A}` → `\subsection{A}` is
   reported only as a DELETION of the section.
10. **AsciiDoc** (fixed by the registry #21 component): the callout example `"Hello world!" // <1>` in a
    listing yielded a node range outside the source, and a link target inside an `image::` macro was missed.

The one failing Svelte migrate pair, `self-closing-elements` (`<div />` → `<div></div>`), is reported with no
changes. The expectation treats every non-whitespace migration as a rewrite that must be shown; for this pair
a style-only answer is also defensible, so it is a contract question rather than a confirmed defect.

### Not claimed

- No robustness/scaling cases. `tsx` is not covered. CSS has only 6 derived pairs (css-parsing-tests inputs
  are tiny); WPT CSS tests were not imported.
- Equivalence oracles are bounded observations (one render, compile or DVI), not semantic equivalence.
- The CommonMark block inventory treats a link reference definition as part of the following block, as
  commonmark.js does; where IntentumDiff reports it as its own node (one case), the domains differ.

## #267 build, configuration and infrastructure family

The family module is `scripts/external_families/family_267_build_infra.py`; its cases are witnessed by
`scripts/build_infra_witness.py` and its bytes come from `scripts/acquire_build_infra_benchmarks.py`. It adds the
core #267 IDs `dockerfile`, `hcl`, `gomod`, `gitignore`, `cmake`, `make` and `puppet`. `bash` and `powershell` are
scripting languages and are left to #266; this family adds no cases for them.

The family adds 776 cases, of which 761 are admitted and executed through the C ABI. Every one of the seven IDs
has a measured parser-fidelity lane and a measured change-understanding lane with witnessed changed and preserved
pairs. No ID has a robustness/scaling result.

It was first published as a separate run in `../2026-11-17-external-scorecard-267/` (worktree `bench-267` on
release/v0.0.3-rc `8898e3f`, library `ced9e0e1…598b`, manifest `23c390a8…`, ledger `1fe0ffce…803c`). That run
gave 622 passed, 128 failed and 11 unsupported; two full runs were identical, the witness step was run twice
with byte-identical records, and all 2,133 earlier cases kept their status. The tables and failures below are
from the combined run on the registry #21 components (see
[Combined run](#combined-run-registry-21-components)): 624 passed, 126 failed and 11 unsupported. The
family's components are `dockerfile_parser.wasm`, `terraform_parser.wasm` (route `hcl`), `gomod_parser.wasm`,
`gitignore_parser.wasm`, `cmake_parser.wasm`, `make_parser.wasm` and `puppet_parser.wasm`; of these, the
dockerfile, gitignore and make components changed, and only make changed a status.

### Sources

Every suite is pinned to a commit. Bytes and SHA-256 are in `upstream/build-infra/<suite>/ACQUISITION.json`,
written by `scripts/acquire_build_infra_benchmarks.py`. Two methods were used:

- **Go module zips.** The tool runs `go mod download -json`, which verifies the zip against sum.golang.org.
  The module's VCS origin gives the commit.
- **Raw files.** Tags and branches were resolved with the Go module proxy's VCS origin record. The files were then
  fetched from `raw.githubusercontent.com/<repo>/<commit>/<path>`.

| Suite (language) | Revision | Licence | Acquisition |
|---|---|---|---|
| BuildKit v0.17.3 `frontend/dockerfile/parser` testfiles (`dockerfile`) | `dc30e8355a4691bb34a23dd7161306977eecd5d8` | Apache-2.0 | module zip `af50411c…`, `h1:XN8ddC5g…` |
| HCL v2.23.0 `specsuite/tests` (`hcl`) | `56a9aee5207dbaed7f061cd926b96fc159d26ea0` | MPL-2.0 | module zip `78c8c335…`, `h1:Fphj1/gC…` |
| Terraform v1.5.7 `internal/configs/testdata/{valid,invalid}-files` (`hcl`) | `ee58ac1851c8a433005df9863ed47796a9f6b5e7` | MPL-2.0 (v1.5.7 predates the BUSL change) | module zip `dbac9050…`, `h1:HL+iEBdp…` |
| golang.org/x/mod v0.22.0 `modfile/testdata` (`gomod`) | `dec0365065b75edd0e98b0306f6f9b0051710ed2` | BSD-3-Clause | module zip `0d0a9605…`, `h1:D4nJWe9z…` |
| Go go1.24.7 `src/cmd/go/testdata/mod` and `script/mod_replace.txt` (`gomod`) | `d41a8222133aabaf5527eab64586050a0fbb773b` | BSD-3-Clause | raw; the 251 testdata/mod files equal the local go1.24.7 GOROOT byte for byte |
| git v2.43.0 `t/t0008-ignores.sh` (`gitignore`) | `564d0252ca632e0264ed670534a51d18a689ef5d` | GPL-2.0 | raw |
| CMake v3.28.3 `Tests/RunCMake/Syntax` and `RunCMake.cmake` (`cmake`) | `5e984bb35232116a54de7db39788cb162ca7c263` | BSD-3-Clause | raw; enumerated from `RunCMakeTest.cmake` |
| GNU Make 4.3 `tests/` (11 scripts plus the harness) (`make`) | `f430a65ccb5e85eb465f6a64afcbdd09f9c708ed` (mirror/make) | GPL-3.0-or-later | raw |
| puppet `spec/unit/pops/parser` (9 specs plus the helper) (`puppet`) | `e227c27540975c25aa22d533a52424a9d2fc886a` (main) | Apache-2.0 | raw; all 11 files are identical at tag 8.10.0 |

**Licences.** git's test script (GPL-2.0) and GNU Make's tests (GPL-3.0-or-later) are vendored as unmodified test
data, with COPYING retained and attribution on every case. The ignore files and makefiles extracted from them keep
the same licence and attribution. None of these bytes are linked into or shipped with IntentumDiff. That is the
basis on which they were vendored; the case records state it as well.

**Pins.** The puppet tag `8.10.0` could not be resolved to a commit, because proxy.golang.org returned not found for
it. The pin is therefore the `main` commit above, and every vendored file was compared byte for byte with the same
path at the tag. The tool used as the oracle is the released gem 8.10.0
(sha256 `f2f5de8f…a3a`, as listed by rubygems.org).

### Oracles (776 witness records, `scripts/build_infra_witness.py witness`)

| Oracle | Tool and version | supports / other |
|---|---|---|
| Dockerfile parse | BuildKit v0.17.3 `parser.Parse`. Must-accept files must reproduce the upstream `result` dump byte for byte. Must-reject files must return an error. | 36 / 0 |
| Dockerfile pairs | BuildKit `instructions.Parse` typed command lists, without locations, original text, the legacy-form marker or keyword spelling | 15 / 1 inconclusive (`health`: the seed's bare `HEALTHCHECK` fails `instructions.Parse`) |
| HCL parse | hcl v2.23.0 `hclsyntax.ParseConfig`. valid-files and specsuite files that expect no diagnostics must be accepted. | 62 / 0 |
| HCL pairs | the hclsyntax information model: attributes as an unordered map of cty-typed constants, or token text for other expressions; blocks as an ordered sequence. `hclwrite.Format` (the formatter behind `terraform fmt`) generated three canonical-format pairs. | 19 / 0 |
| go.mod parse | x/mod v0.22.0 `modfile.Parse` (strict) and cmd/go go1.24.7 `go mod edit -json` | 139 / 10 contradict (5 x/mod format-only fixtures such as `block`, `comment` and `rule1` are not valid go.mod; 5 cmd/go testdata members are intentionally bad) |
| go.mod pairs | `go mod edit -json` equality | 17 / 2 inconclusive (`block`, `comment` goldens do not parse) |
| gitignore | git 2.43.0. The setup files reproduce t0008's `--stdin -v` expected-verbose output. The `**` file reproduces its `check-ignore` expectation, and the backslash file its `ls-files -X` expectation. Pairs compare `check-ignore --no-index -z --stdin` sets over a fixed universe of materialised files. | 19 / 0 |
| cmake | the upstream RunCMake harness on cmake 3.28.3 (Unix Makefiles). Pairs compare the configure exit status, stdout and stderr with paths and timings normalised. | 118 / 2 inconclusive (`AtWithVariable*File` need an input file that is not vendored) |
| make | the upstream `run_make_tests.pl` harness on GNU Make 4.3 and Perl 5.38.2; it reproduced all 70 makefile expectations. Pairs compare `make` exit status, stdout and stderr. | 78 / 0 |
| puppet | puppet 8.10.0 (Ruby 3.3.6) `Parser#parse_string`, which is what the specs' `parse` helper calls. Pairs compile a catalog for a fixed node and normalise resources plus ordering/refresh edges. | 258 / 0 |

Only `supports_claim` admits a case. Two rules were adjusted after the first witness run, before any engine run:

- **Keyword spelling.** The Dockerfile pair projection first included the instruction keyword as written, so `ADD`
  → `add` contradicted the claim that instructions are case-insensitive. BuildKit dispatches keywords
  case-insensitively, so the spelling is no longer observed. The `type` field still identifies the command.
- **Expansion-time make errors.** The make must-reject rule first counted `unterminated variable reference`, which
  GNU Make raises when a variable is expanded, not when the line is read. Executed check:
  `x = $(foo` / `all: ; @echo hi` prints `hi` and exits 0 on make 4.3. `variables/negative` tests 0 and 1 are
  therefore must-accept.

#### Parser-fidelity claims

- **dockerfile:** BuildKit's own must-accept and must-reject split. `testfiles-negative/empty_dockerfile` and
  `only_comments` are excluded. Their only upstream error is "file with no instructions", which describes an empty
  document, not a syntax error.
- **hcl:** native-syntax acceptance. Terraform invalid-files whose error is a schema or semantic error are valid HCL
  and must be accepted; those with syntax errors must be rejected. `.tf.json` files are not routed to `hcl`.
- **cmake:** must-reject when the upstream expected stderr reports `Parse error` (or `Syntax Error in cmake code`)
  in the file. Configure-time errors such as an unmatched `function()` or `${` in an argument leave the file
  must-accept.
- **make:** must-reject when the upstream expected output is a makefile read error, such as a missing separator, an
  empty variable name or a missing `endef`.
- **puppet:** the spec's own expectation, either `expect(...)` success or `expect { parse(...) }.to raise_error`.
- **Statement inventories:** only Dockerfile, HCL and go.mod documents without comments carry one, taken from the
  reference's unit start bytes. The comparison domain cannot tell a comment node from a statement without node type
  names.

### Cases per language × lane × class (admitted / executed / passed / failed / unsupported)

| Language | Lane | Class | Admitted | Passed | Failed | Unsupported | Candidates |
|---|---|---|---:|---:|---:|---:|---:|
| dockerfile | parser | upstream_gold | 36 | 18 | 18 | 0 | – |
| dockerfile | change | reviewed_external_derived | 15 | 6 | 0 | 9 | 1 |
| hcl | parser | upstream_gold | 62 | 55 | 7 | 0 | – |
| hcl | change | reviewed_external_derived | 19 | 16 | 3 | 0 | – |
| gomod | parser | upstream_gold | 139 | 137 | 2 | 0 | 10 |
| gomod | change | upstream_gold (x/mod .in→.golden) | 6 | 1 | 4 | 1 | 2 |
| gomod | change | reviewed_external_derived | 11 | 9 | 2 | 0 | – |
| gitignore | parser | reviewed_external_derived (here-document payloads) | 7 | 7 | 0 | 0 | – |
| gitignore | change | reviewed_external_derived | 12 | 7 | 5 | 0 | – |
| cmake | parser | upstream_gold | 108 | 71 | 37 | 0 | 2 |
| cmake | change | reviewed_external_derived | 10 | 6 | 3 | 1 | – |
| make | parser | upstream_gold | 60 | 54 | 6 | 0 | – |
| make | parser | reviewed_external_derived (harness-substituted) | 10 | 10 | 0 | 0 | – |
| make | change | reviewed_external_derived | 8 | 7 | 1 | 0 | – |
| puppet | parser | upstream_gold | 235 | 197 | 38 | 0 | – |
| puppet | parser | reviewed_external_derived (escaped literals) | 16 | 16 | 0 | 0 | – |
| puppet | change | reviewed_external_derived | 7 | 7 | 0 | 0 | – |

#### Results per language × lane

| Language | Lane | Valid accepted | Invalid rejected | Change recall | Preservation false alarms |
|---|---|---|---|---|---|
| dockerfile | parser | 16/34 | 2/2 | N/A | N/A |
| dockerfile | change | N/A | N/A | 3/3 | 0/3 (9 unsupported) |
| hcl | parser | 53/55 | 2/7 | N/A | N/A |
| hcl | change | N/A | N/A | 9/9 | 3/10 |
| gomod | parser | 137/139 | N/A | N/A | N/A |
| gomod | change | N/A | N/A | 6/7 | 5/9 |
| gitignore | parser | 7/7 | N/A | N/A | N/A |
| gitignore | change | N/A | N/A | 6/6 | 5/6 |
| cmake | parser | 65/88 | 6/20 | N/A | N/A |
| cmake | change | N/A | N/A | 2/4 | 1/5 |
| make | parser | 64/65 | 0/5 | N/A | N/A |
| make | change | N/A | N/A | 4/4 | 1/4 |
| puppet | parser | 212/234 | 1/17 | N/A | N/A |
| puppet | change | N/A | N/A | 3/3 | 0/4 |

### Real failures, minimised (recorded, not filed)

Each reproducer was rechecked directly through `live_diff_contents`, using the same transport and library as the
runner.

**dockerfile (`dockerfile_parser.wasm`)**

1. **Legacy forms fall back.** Each of these gives `native_parse_incomplete`, while `LABEL a=b` and `ENV a b` are
   fine:
   - the legacy `LABEL a b`;
   - the legacy `ENV a b c` with a multi-word value;
   - `ARG a b`, which declares two names;
   - a trailing continuation at EOF (`RUN a \` with no newline);
   - `ENV name='a\'b''`.

   This is 16 of the 34 must-accept upstream files and all 9 unsupported change cases (their seeds contain a
   legacy `LABEL` or `ENV`).
2. **Instructions are dropped or split.** BuildKit treats an unterminated JSON array
   (`CMD ["echo", "Please, close the brackets when you're done"`) as shell form. The first run's tree had no
   instruction node; on the registry #21 component the file falls back with `native_parse_incomplete`. In `health`, the multi-line `HEALTHCHECK … \` continuation yields 12 units against BuildKit's 10.

**hcl (`terraform_parser.wasm`)**

3. **Identity invents changes for repeated blocks.** With the identical input `a {\n  x = 1\n}\na {\n  x = 2\n}\n` on
   both sides, the attributes are cross-matched and reported as a MODIFICATION of `x` (Terraform `provider-configs.tf`
   and `providers-explicit-implied.tf`).
4. **Unclosed and invalid syntax is accepted silently.** `a {` reviews as style-only with no diagnostics. The same
   happens to `version = ` with no value and to `${…}` interpolation in a block label.
5. **Reorders are not equivalent.** Reordering an object constructor's keys (`{"a" = 1, "b" = 2}` → `{"b" = 2,
   "a" = 1}`) is reported as a MODIFICATION of the attribute. Swapping two attributes (`a = 1\nb = 2` → `b = 2\na = 1`)
   returns `completeness: incomplete`.

**gomod (`gomod_parser.wasm`)**

6. **`// indirect` is missed.** `require a.b/c v1.0.0` → `require a.b/c v1.0.0 // indirect` is style-only, but cmd/go
   reports `Indirect: true`.
7. **Quoting is a MODIFICATION.** `module "x"` → `module x` and `replace "a.b/c" v1.0.0 => "/tmp/z"` → the unquoted
   form are reported as MODIFICATIONs of `module_path`/`file_path`, although `go mod edit -json` is equal. Four of
   the five x/mod `.in`→`.golden` format pairs fail this way (`module`, `retract`, `replace2`; `empty` is a
   completeness mismatch).
8. **Fallback on quoted paths.** `replace (\n\tw v1.0.0 => "./a,"\n)` falls back on the valid x/mod
   `replace.in`/`replace.golden`.

**gitignore (`gitignore_parser.wasm`)**

9. **Comment lines are code edits.** Adding `# c` before `one` is a semantic ADDITION of a `comment` node, and
   removing comment lines is a DELETION (3 false alarms).
10. **Reordering returns `incomplete`.** Swapping two patterns with disjoint matches (`one\ntwo` → `two\none`), or
    two negations, returns `completeness: incomplete`.

**cmake (`cmake_parser.wasm`)**

11. **Parse errors are accepted silently.** 14 of 20 must-reject files are affected. Minimal inputs:
    - `message("Example Message") message("Second Message")` (no separator between commands);
    - `message(STATUS [[bracket]]unquoted)`;
    - `CommandEOF`, `ParenNoSpace1`, `UnterminatedBracketComment` and `NullTerminatedArgument`.
12. **Identity invents changes.** `message(${var with space})` and `message("-->${e(x)}<--")` on both sides report an
    ADDITION of `normal_command` labelled `message({var)` / `message(-->$)` (10 files, among them the
    must-reject `CommandError0`).
13. **Fallback on files CMake parses.** Several files fall back although CMake parses them:
    - `function(f)` or `foreach(i 1 2)` without its end command;
    - `set(var "${")` and `message("${var with space}")`;
    - `Bracket1`'s multi-line `[=[…]==]=]`;
    - `CMP0053-NUL`.

    CMake reports configure-time errors for some of these, not parse errors.
14. **Missed value changes.** `message(a " " b)` → `message(a "  " b)` is style-only, and so is
    `message("a \${v} 4")` → `… 5`, which has an escaped `$` in the string. Both change the output.
15. **Command case is a MODIFICATION.** `message(a)` → `MESSAGE(a)` is reported as a MODIFICATION, but command names
    are case-insensitive.

**make (`make_parser.wasm`)**

16. **Flavor spellings.** In the first run these flavor changes were style-only with
    `completeness: equivalent`, although GNU Make prints different output for each:
    - `X = $(Y)` → `X := $(Y)` (recursive to simple);
    - `foo := $(bar)` → `foo = $(bar)`;
    - `foo += $(bar)` → `foo = $(bar)`.

    The registry #21 component (make-parser#9) reports each as a MODIFICATION of `assignment_operator`, so all
    three pass. It now also reports `:=` → `::=` as a MODIFICATION, although GNU Make treats the two
    identically (`gmake-variables-flavors-01-equivalent-d51f29`, a new preservation false alarm).
17. **Read errors are accepted silently.** All 5 must-reject makefiles review without diagnostics: `$x.` (missing
    separator), 8 spaces instead of a tab, an empty variable name and an unterminated `define`.
18. **Fallback on a valid conditional.** `variables/define` test 3 falls back on valid input. It contains
    `ifdef BOGUS` around `define $(subst e,e,$(NAME)) =`, with an `ifeq`/`endif` in the body, and GNU Make prints
    `foo`. Smaller variants do not reproduce it, so the 14-line file is the reproducer.

**puppet (`puppet_parser.wasm`)**

19. **Valid manifests fall back (22 of 234).** Minimal examples:
    - `! 'x'`, `[]`, `$a = [1,2,3][2]`, `$a -= 10` and `$a.foo`;
    - `unless true { $a = 10 } else {$a = 20}`;
    - `@File[foo] { mode => '0777' }`;
    - `class { 'cname1': x => 1; 'cname2': y => 2}`;
    - `File <| tag == 'foo' and mode != '0777' |>`.
20. **Invalid manifests are accepted (16 of 17).** Examples are `class class {}`, `type A = B[]`,
    `notice a => 2` and `@file {: mode => '0777' }`.

### Blocked or left as a gap

- **IaC-Eval (`hcl`, the selected primary source): BLOCKED.** huggingface.co refuses the CONNECT tunnel (403),
  so no revision-pinned bytes could be hashed. The dataset card (read through the Hugging Face MCP) says CC-BY-4.0.
  It is recorded in `blocked_acquisitions`. HCL is measured from the HCL spec suite and Terraform testdata.
- **Shipwright (`dockerfile`, the selected primary source): BLOCKED.**
  - No commit could be resolved: `git ls-remote` was refused by the permission policy, github.com and its API are
    unreachable, and the Go proxy returns not found.
  - Its build oracle needs a Docker daemon, which is not available.
  - It is recorded in `blocked_acquisitions`. Dockerfile change cases are BuildKit-derived instead.
- **No image builds.** There is no Docker daemon and BuildKit has no worker, so Dockerfile change pairs are judged
  at the parsed-instruction level only. No image config or runtime is observed.
- **No `terraform validate`/`plan`.** Providers would need registry.terraform.io, and no Terraform binary was
  installed. HCL claims are syntax and information-model claims, not Terraform semantics.
- **gomod:** `go build`/`go run` witnesses (the mod_replace runtime output) were not run, because they need the
  module proxy test server. The 2 `block`/`comment` golden pairs and 10 invalid or format-only documents stay
  candidates.
- **cmake:** `AtWithVariableFile`/`AtWithVariableAtOnlyFile` need an input file that is not vendored, so they stay
  candidates. Script-mode (`-P`) tests and other RunCMake directories are not imported.
- **make:** only 11 scripts are imported. Expected outputs were reproduced as root on Linux.
- **puppet:** `compiler_spec.rb` catalog specs and the OOPSLA 2024 fault corpus were not imported. Interpolated and
  heredoc spec literals are not extracted. Change pairs compile catalogs only, and no provider is run.
- **bash/powershell:** left to #266.
- **Robustness/scaling lane:** no admitted cases for any of the seven IDs.

### Not claimed

- Instruction-list, information-model, catalog or output equality on the exercised inputs is not program or
  build equivalence.
- Derived pairs are reviewed transformations of upstream files, not upstream edit benchmarks.
- No pooled or macro average is computed.
