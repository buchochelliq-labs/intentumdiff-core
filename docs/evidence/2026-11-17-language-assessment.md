# Language assessment with the shipped 0.0.3b1 components — 2026-11-17

This is a bounded diagnostic run of all **760 source-judged probes across all 74 registered routes**. It used the native Rust C ABI built from `release/v0.0.3-rc` head `74618c311836983b4f60ae2d8389edfdd14f7793`. It used the **73 Wasm components shipped in the verified Python 0.0.3b1 wheel**, without rebuilding them or substituting registry pins. It is not full-language certification.

The complete runner report is [`2026-11-17-language-assessment.json`](2026-11-17-language-assessment.json). It is the exact runner output, so it can serve as a baseline input; SHA256 `02bd6e7e6bdf875274c3bb97bd33758182f230b4744d65d8e4410793df066847`.

## Headline

| | passed | failed | invalid | incomplete | explicit fallback |
|---|---:|---:|---:|---:|---:|
| **This run (shipped components)** | **591** | **91** | 3 | 43 | 32 |
| Last published, #295 (2026-11-09, RC registry pins) | 596 | 86 | 3 | 43 | 32 |
| Protected baseline, #291 (584-pass) | 584 | 98 | 3 | 43 | 32 |

All 760 cases ran: 0 were not executed and there were 0 worker errors. Every one of the 74 routes has a staged component, so no route is missing one. `freebasic_parser.wasm` is shipped but no route uses it.

## REGRESSIONS: the shipped wheel uses stale parser components

> **The protected-baseline gate FAILS with 5 regressions. Against the last published run (#295) there are 17 regressions.**
> In every one, a real edit is reported as *no semantic change* (`has_semantic_changes: expected True, got False`), except the two EOF-identity cases, which return an out-of-source range.
> The cause is component staleness, not the core engine. The 0.0.3b1 wheel stages components from registry commit `8911328` (registry PR #17, 2026-11-08 02:32). That predates registry #18 (complete-literal JS/TS, JSON, SQL and YAML) and #19 (ADF and Databricks).

| Route | Case | vs protected | vs #295 | Shipped component | RC registry pin |
|---|---|---|---|---|---|
| javascript | `dynamic/javascript/literal-tail-after-4096` | **regression** | **regression** | js_ts `3677a9f5…` (eacbac4) | `55b6c5d0…` |
| typescript | `dynamic/typescript/literal-tail-after-4096` | **regression** | **regression** | js_ts `3677a9f5…` | `55b6c5d0…` |
| json | `data_config.json.known_long_literal_suffix` | **regression** | **regression** | `8edbe1a6…` (2bc6d7f) | `ffb886cf…` |
| sql | `data_config.sql.known_long_literal_suffix` | **regression** | **regression** | `4dc57fb8…` (16fd12e) | `10255c58…` |
| yaml | `data_config.yaml.known_long_literal_suffix` | **regression** | **regression** | `6b0f3802…` (74b1af3) | `2ff91eb6…` |
| adf | `known_retry_policy`, `meaningful_1_value`, `meaningful_3_body`, `unicode_value` | not protected | **4 regressions** | `941113f3…` (c74e01a, 2026-10-07) | `d1537ca4…` (cf3f269) |
| databricks | `known_base_parameter`, `known_eof_identity`, `known_notebook_payload`, `meaningful_2_configuration` | not protected | **4 regressions** | `ea55a639…` (0b09ca4) | `b1ba6fb8…` |
| databricks-workflow | same four cases | not protected | **4 regressions** | `ea55a639…` | `b1ba6fb8…` |

**Attribution control.** I re-ran the eight affected routes (90 cases) with the same native library, swapping in only the six RC-registry-pinned components (byte-identical to the #295 manifest). **All 17 regressions pass again**, and those eight routes reproduce #295's statuses exactly (0 differences). No regression is attributable to the core head.

**Consequence.** Users of the shipped 0.0.3b1 wheel get no semantic change for these ADF/Databricks value edits and long-literal suffix edits. The wheel must be rebuilt from a registry commit at or after #19, and then from the re-pins below, before 0.0.3 release.

## Improvements

All 12 are core-engine fixes merged since #295. They improve on both the protected baseline and #295:

- xml: `data_config.xml.known_text_whitespace`, `data_config.xml.meaningful_3_significant_whitespace`
- perl: `dynamic/perl/spacing`
- astro: `web-astro-preservation`
- html: `web-html-pre_whitespace`
- markdown: `web-markdown-external-code-block-form`, `web-markdown-external-heading-tab`, `web-markdown-heading_level`, `web-markdown-preservation`
- mdx: `web-mdx-external-closing-heading-marker`, `web-mdx-heading_level`
- svelte: `web-svelte-preservation`

#295's 12 improvements were the 12 ADF/Databricks cases above, which regress here only because of the stale components. With the RC pins, the projected total for this core head is **608 passed / 74 failed** (591 + 17).

## The three manual explanation findings from the 2026-11-08 assessment

| Case | 2026-11-08 defect | This run (shipped) | Status |
|---|---|---|---|
| `web-tsx-external-module-quote-spelling` | evidence span [23,30] instead of [144,151] (core#277) | `IGNORED_STYLE` with `old_span`/`new_span` = [144,151] | **Fixed** (core #282/#308) |
| `dynamic/elixir/language-feature` | `def`→`defp` explained as `RENAME_VARIABLE` (core#271) | one `MODIFICATION` change, "Update identifier('def') -> identifier('defp')", in one meaningful group; no rename | **Fixed** (core #305). The wording names the keyword token rather than a public→private visibility change; this is a precision observation, not the logged defect |
| `abap-unicode-payload` | backtick string `café`→`cafe雪` explained as `RENAME_VARIABLE CAF -> CAFE` (abap-parser#8) | no rename, but a spurious second change "Update identifier('CAF') -> identifier('CAFE')" (cols 7..10) and a separate `CAF`→`CAFE` meaningful group next to the correct `write_statement` change | **Still present in the shipped component.** The shipped abap component is `832e64a` (2026-11-01); the fix abap-parser#9 merged 2026-11-11 and is not pinned. A local probe build of abap-parser RC head `08b9fce` gives exactly one change, `string_literal('`café`') -> string_literal('`cafe雪`')`, and 9/9 oracle-judged ABAP cases pass |

## Component staleness beyond the failing cases

The registry `release/v0.0.3-rc` head is still #19 (2026-11-09). This session could read 44 of the shipped parser repos (43 routed plus freebasic). Every one has later commits on `release/v0.0.3-rc` than its shipped component, by 2 to 16 commits. That includes the merged fixes for all 22 parser-repo groups logged in core#280 (asm, assemblyscript, csharp, rust, kotlin, swift, abap, groovy, ocaml, reasonml, php, powershell, bash, astro, postscript, svelte, js-ts, dockerfile, yaml, plsql and tsql). Those fixes are **not exercised by this run or by any shipped artifact**. The other 25 parser repos are not visible to this session. Re-pin the registry from the CI artifacts of each repo's RC head, rebuild the wheel, then re-run this assessment.

## Reproduction

| Input | Identity |
|---|---|
| Core source | `74618c311836983b4f60ae2d8389edfdd14f7793`, tree `74eae0217b0aea5c84657909a7f0114940ceeeb3`, `crates/rust-core-host` tree `d3fc4a30369f2e5770569c86c1199f2fe98682c9` |
| Runner `scripts/assess_languages.py` blob | `dcede31678e23817c0df7331ced52d8970985941` (unchanged) |
| Native library | `cargo +1.95.0 build --release --locked --lib`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_RELEASE_DEBUG=0`; SHA256 `2527e2e560b40f7ec2abfeaf8ad90f559dafde7fb4f289dabc78e028a5c5f02e` |
| Components | 73 Wasm from the 0.0.3b1 wheel: wheel SHA256 `d4007d47ee9e59b4087b355adeab3f933b871adc4f31edacf96ad969a214f070`, archive `dad786b7…`, workflow run 44059484269, python commit `767dbfb`, registry commit `8911328ede24630ad9ea6a33da8ed010f066d3b1`. Every byte matches the wheel's `wasm_provenance.json` |
| Stage manifest | the wheel's `parser_manifest.json` (routing equals the fixed inventory) plus a `components` list derived from `wasm_provenance.json`; SHA256 `a9e78b5d53ba3c82d151c5c9bf93bb834c513cd4765b247a373b34c458193521` (embedded in the JSON) |
| Inventory | SHA256 `37f27f7b6ea8c21b6f7af3f07a189d0c76d73ab8ea316f96eea20ef5559db4d7` |
| Gate baseline | `2026-11-08-merged-rc-language-baseline.json`, SHA256 `178addef636347d966ab128979f5bcad0139a06d9f207de1d381f45cc654c704` |
| Run | `python scripts/assess_languages.py --fixtures tests/fixtures/language_assessment --parsers <stage> --native-library <lib> --baseline docs/evidence/2026-11-08-merged-rc-language-baseline.json --report <out>/results.json --jobs 4`; exit 1 (gate: 5 regressions, 12 improvements); run start 2026-11-17T19:56:41Z |

The per-route raw outputs (17 MB) are not committed; the JSON retains every case result. The control run and the ABAP probe were diagnostic only and are not part of the JSON.

## Scope limits

Unchanged from earlier assessments. Passing finite probes does not certify a language. Exact-span accuracy, explanation precision/recall, dialect/platform matrices, fuzzing and sustained load remain unassessed (#156, #261, #268). The 91 failed, 3 invalid, 43 incomplete and 32 fallback results stay open for repair and adjudication.
