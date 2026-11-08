# Language assessment — 8 November 2026

Bounded diagnostic baseline across all **74 registered language IDs** using the production Rust C ABI and explicitly selected parser components. This report identifies reproducible defects; it is not an accuracy certificate or complete language conformance assessment.

Repair parent tracker: [core #280](https://github.com/buchochelliq-labs/intentumdiff-core/issues/280). Fixtures, runner and guide were published at immutable core commit `21ce7edf4f27ffd0626da19ca6629df8fbc2af9a` in [PR #269](https://github.com/buchochelliq-labs/intentumdiff-core/pull/269).

## Results

**760 cases executed: 559 probes passed their declared verdict and validity checks, 123 failed classifications/valid-source checks, 3 invalid results and 75 bounded incomplete-input outcomes.** The 126 automated failures affect 55 IDs. Three independently reviewed explanation/evidence defects among flags-passing cases bring the confirmed total to **129 case findings across 56 IDs**.

**Case classes:** 488 handwritten regressions, 169 repository fixtures, 103 external-derived cases and 0 upstream-gold cases. The 760 per-language-ID executions contain **732 unique old/new byte pairs**; aliases retain separate rows and explicit parser routing.

Of the automated failures, 81 cases link to **34 existing issues freshly verified open**, and 45 cases link to newly filed issues. The three manual findings also link to new issues: **32 new issue groups** in total, also open. The 119 classification failures, 6 source-range failures and 2 group-reference failures overlap; do not add those lane counts.

## Candidate identity

- Native C ABI SHA-256: `4bd34bb25a463d68f89a5aa536a4fd694b43261969972c75b3e73332e1bf94bd`.

- Registry inventory SHA-256: `37f27f7b6ea8c21b6f7af3f07a189d0c76d73ab8ea316f96eea20ef5559db4d7`.

- Parser manifest SHA-256: `2be61b459c125dbaa28c55bffaff7a3ac56a558c24f933650ffd19094e4332b9`. The accompanying JSON retains the complete original component manifest, source commits, artifact identities and all reported fixture hashes.

- Original run UTC: `2026-11-08T15:38:22.359296+00:00`; interface: production Rust C ABI: live_diff_contents + parse_to_tree.

## Per-language scorecard

Pass means the probe passed its declared verdict and validity checks, not external benchmark admission. Bounded means an incomplete-input observation or explicit fallback, not a correctness pass. Manual findings are additional source-reviewed explanation/evidence defects among probes that passed their declared checks. External is the declared external-derived case count; all source admission remains incomplete.

| Language ID | Cases | Pass | Fail | Invalid | Bounded | Manual | External | Source admission | Confirmed issue links |
|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| `abap` | 10 | 7 | 2 | 0 | 1 | 1 | 0 | no external source | [intentumdiff-abap-parser#7](https://github.com/buchochelliq-labs/intentumdiff-abap-parser/issues/7), [intentumdiff-abap-parser#8](https://github.com/buchochelliq-labs/intentumdiff-abap-parser/issues/8) |
| `adf` | 8 | 3 | 4 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-adf-parser#7](https://github.com/buchochelliq-labs/intentumdiff-adf-parser/issues/7) |
| `asciidoc` | 10 | 7 | 2 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-asciidoc-parser#6](https://github.com/buchochelliq-labs/intentumdiff-asciidoc-parser/issues/6) |
| `asm` | 9 | 6 | 2 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-asm-parser#4](https://github.com/buchochelliq-labs/intentumdiff-asm-parser/issues/4) |
| `assemblyscript` | 10 | 8 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-assemblyscript-parser#5](https://github.com/buchochelliq-labs/intentumdiff-assemblyscript-parser/issues/5) |
| `astro` | 10 | 5 | 4 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-astro-parser#17](https://github.com/buchochelliq-labs/intentumdiff-astro-parser/issues/17), [intentumdiff-core#272](https://github.com/buchochelliq-labs/intentumdiff-core/issues/272) |
| `bash` | 11 | 9 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-bash-parser#4](https://github.com/buchochelliq-labs/intentumdiff-bash-parser/issues/4) |
| `c` | 13 | 10 | 2 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-cpp-parser#4](https://github.com/buchochelliq-labs/intentumdiff-cpp-parser/issues/4) |
| `clojure` | 11 | 10 | 0 | 0 | 1 | 0 | 2 | candidate only | — |
| `cmake` | 7 | 6 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `cpp` | 18 | 15 | 2 | 0 | 1 | 0 | 4 | candidate only | [intentumdiff-cpp-parser#4](https://github.com/buchochelliq-labs/intentumdiff-cpp-parser/issues/4) |
| `csharp` | 10 | 8 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-csharp-parser#4](https://github.com/buchochelliq-labs/intentumdiff-csharp-parser/issues/4) |
| `css` | 10 | 7 | 2 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#254](https://github.com/buchochelliq-labs/intentumdiff-core/issues/254), [intentumdiff-core#273](https://github.com/buchochelliq-labs/intentumdiff-core/issues/273) |
| `dart` | 9 | 8 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `databricks` | 10 | 5 | 4 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-databricks-parser#7](https://github.com/buchochelliq-labs/intentumdiff-databricks-parser/issues/7), [intentumdiff-databricks-parser#8](https://github.com/buchochelliq-labs/intentumdiff-databricks-parser/issues/8) |
| `databricks-workflow` | 10 | 5 | 4 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-databricks-parser#7](https://github.com/buchochelliq-labs/intentumdiff-databricks-parser/issues/7), [intentumdiff-databricks-parser#8](https://github.com/buchochelliq-labs/intentumdiff-databricks-parser/issues/8) |
| `dax` | 7 | 6 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `delphi` | 9 | 8 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `dockerfile` | 8 | 5 | 2 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-dockerfile-parser#4](https://github.com/buchochelliq-labs/intentumdiff-dockerfile-parser/issues/4), [intentumdiff-dockerfile-parser#5](https://github.com/buchochelliq-labs/intentumdiff-dockerfile-parser/issues/5) |
| `elixir` | 11 | 10 | 0 | 0 | 1 | 1 | 2 | candidate only | [intentumdiff-core#271](https://github.com/buchochelliq-labs/intentumdiff-core/issues/271) |
| `generic` | 8 | 7 | 0 | 1 | 0 | 0 | 0 | no external source | [intentumdiff-generic-parser#4](https://github.com/buchochelliq-labs/intentumdiff-generic-parser/issues/4) |
| `gitignore` | 10 | 8 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-gitignore-parser#4](https://github.com/buchochelliq-labs/intentumdiff-gitignore-parser/issues/4) |
| `go` | 12 | 11 | 0 | 0 | 1 | 0 | 3 | candidate only | — |
| `gomod` | 7 | 6 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `graphql` | 10 | 5 | 4 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-graphql-parser#8](https://github.com/buchochelliq-labs/intentumdiff-graphql-parser/issues/8) |
| `groovy` | 11 | 8 | 2 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-groovy-parser#4](https://github.com/buchochelliq-labs/intentumdiff-groovy-parser/issues/4) |
| `haskell` | 11 | 10 | 0 | 0 | 1 | 0 | 2 | candidate only | — |
| `hcl` | 14 | 12 | 0 | 0 | 2 | 0 | 7 | candidate only | — |
| `html` | 10 | 8 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#247](https://github.com/buchochelliq-labs/intentumdiff-core/issues/247) |
| `ini` | 7 | 6 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `java` | 13 | 12 | 0 | 0 | 1 | 0 | 3 | candidate only | — |
| `javascript` | 12 | 10 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-js-ts-parser#8](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/8) |
| `json` | 15 | 12 | 1 | 0 | 2 | 0 | 7 | candidate only | [intentumdiff-json-parser#4](https://github.com/buchochelliq-labs/intentumdiff-json-parser/issues/4) |
| `kotlin` | 9 | 7 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-kotlin-parser#5](https://github.com/buchochelliq-labs/intentumdiff-kotlin-parser/issues/5) |
| `latex` | 10 | 4 | 5 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-latex-parser#4](https://github.com/buchochelliq-labs/intentumdiff-latex-parser/issues/4) |
| `lua` | 11 | 10 | 0 | 0 | 1 | 0 | 2 | candidate only | — |
| `m` | 9 | 5 | 3 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-dax-parser#11](https://github.com/buchochelliq-labs/intentumdiff-dax-parser/issues/11) |
| `make` | 8 | 4 | 3 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-core#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278), [intentumdiff-make-parser#4](https://github.com/buchochelliq-labs/intentumdiff-make-parser/issues/4) |
| `markdown` | 10 | 5 | 4 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#274](https://github.com/buchochelliq-labs/intentumdiff-core/issues/274), [intentumdiff-core#275](https://github.com/buchochelliq-labs/intentumdiff-core/issues/275) |
| `mdx` | 10 | 4 | 5 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#275](https://github.com/buchochelliq-labs/intentumdiff-core/issues/275), [intentumdiff-mdx-parser#7](https://github.com/buchochelliq-labs/intentumdiff-mdx-parser/issues/7) |
| `ocaml` | 11 | 8 | 2 | 0 | 1 | 0 | 2 | blocked only (2 blocked) | [intentumdiff-ocaml-parser#7](https://github.com/buchochelliq-labs/intentumdiff-ocaml-parser/issues/7), [intentumdiff-ocaml-parser#8](https://github.com/buchochelliq-labs/intentumdiff-ocaml-parser/issues/8) |
| `odin` | 9 | 7 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-odin-parser#4](https://github.com/buchochelliq-labs/intentumdiff-odin-parser/issues/4) |
| `perl` | 11 | 9 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#270](https://github.com/buchochelliq-labs/intentumdiff-core/issues/270) |
| `php` | 11 | 9 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-php-parser#7](https://github.com/buchochelliq-labs/intentumdiff-php-parser/issues/7) |
| `plsql` | 8 | 5 | 2 | 1 | 0 | 0 | 0 | no external source | [intentumdiff-plsql-parser#4](https://github.com/buchochelliq-labs/intentumdiff-plsql-parser/issues/4), [intentumdiff-plsql-parser#5](https://github.com/buchochelliq-labs/intentumdiff-plsql-parser/issues/5) |
| `po` | 10 | 9 | 0 | 0 | 1 | 0 | 2 | candidate only | — |
| `postscript` | 8 | 4 | 3 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-postscript-parser#6](https://github.com/buchochelliq-labs/intentumdiff-postscript-parser/issues/6), [intentumdiff-postscript-parser#7](https://github.com/buchochelliq-labs/intentumdiff-postscript-parser/issues/7) |
| `powershell` | 11 | 8 | 2 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-powershell-parser#8](https://github.com/buchochelliq-labs/intentumdiff-powershell-parser/issues/8) |
| `proto` | 7 | 5 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-core#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278) |
| `puppet` | 7 | 5 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-core#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278) |
| `python` | 11 | 9 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#243](https://github.com/buchochelliq-labs/intentumdiff-core/issues/243), [intentumdiff-core#254](https://github.com/buchochelliq-labs/intentumdiff-core/issues/254) |
| `qsharp` | 10 | 4 | 5 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-qsharp-parser#8](https://github.com/buchochelliq-labs/intentumdiff-qsharp-parser/issues/8) |
| `r` | 11 | 10 | 0 | 0 | 1 | 0 | 2 | blocked only (2 blocked) | — |
| `reasonml` | 11 | 8 | 2 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-reasonml-parser#7](https://github.com/buchochelliq-labs/intentumdiff-reasonml-parser/issues/7), [intentumdiff-reasonml-parser#8](https://github.com/buchochelliq-labs/intentumdiff-reasonml-parser/issues/8) |
| `ruby` | 11 | 10 | 0 | 0 | 1 | 0 | 2 | candidate only | — |
| `rust` | 15 | 11 | 3 | 0 | 1 | 0 | 3 | candidate only | [intentumdiff-rust-parser#4](https://github.com/buchochelliq-labs/intentumdiff-rust-parser/issues/4), [intentumdiff-rust-parser#5](https://github.com/buchochelliq-labs/intentumdiff-rust-parser/issues/5) |
| `sas` | 10 | 6 | 3 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-sas-parser#9](https://github.com/buchochelliq-labs/intentumdiff-sas-parser/issues/9) |
| `scala` | 9 | 8 | 0 | 0 | 1 | 0 | 0 | no external source | — |
| `scss` | 10 | 7 | 2 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#276](https://github.com/buchochelliq-labs/intentumdiff-core/issues/276) |
| `sql` | 8 | 6 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-sql-parser#4](https://github.com/buchochelliq-labs/intentumdiff-sql-parser/issues/4) |
| `squirrel` | 11 | 10 | 0 | 0 | 1 | 0 | 2 | candidate only | — |
| `svelte` | 10 | 4 | 5 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-core#272](https://github.com/buchochelliq-labs/intentumdiff-core/issues/272), [intentumdiff-svelte-parser#7](https://github.com/buchochelliq-labs/intentumdiff-svelte-parser/issues/7), [intentumdiff-svelte-parser#8](https://github.com/buchochelliq-labs/intentumdiff-svelte-parser/issues/8) |
| `swift` | 9 | 7 | 1 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-swift-parser#5](https://github.com/buchochelliq-labs/intentumdiff-swift-parser/issues/5) |
| `toml` | 14 | 11 | 1 | 0 | 2 | 0 | 7 | candidate only | [intentumdiff-core#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278) |
| `tsql` | 8 | 5 | 2 | 1 | 0 | 0 | 0 | no external source | [intentumdiff-tsql-parser#4](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/4), [intentumdiff-tsql-parser#5](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/5), [intentumdiff-tsql-parser#6](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/6) |
| `tsx` | 10 | 8 | 1 | 0 | 1 | 1 | 2 | candidate only | [intentumdiff-core#277](https://github.com/buchochelliq-labs/intentumdiff-core/issues/277), [intentumdiff-js-ts-parser#9](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/9) |
| `typescript` | 12 | 10 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-js-ts-parser#8](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/8) |
| `vbnet` | 11 | 8 | 2 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-vbnet-parser#6](https://github.com/buchochelliq-labs/intentumdiff-vbnet-parser/issues/6) |
| `vue` | 10 | 2 | 7 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-vue-parser#6](https://github.com/buchochelliq-labs/intentumdiff-vue-parser/issues/6) |
| `wast` | 13 | 11 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-wat-parser#4](https://github.com/buchochelliq-labs/intentumdiff-wat-parser/issues/4) |
| `wat` | 13 | 11 | 1 | 0 | 1 | 0 | 2 | candidate only | [intentumdiff-wat-parser#4](https://github.com/buchochelliq-labs/intentumdiff-wat-parser/issues/4) |
| `xml` | 8 | 5 | 2 | 0 | 1 | 0 | 0 | no external source | [intentumdiff-core#247](https://github.com/buchochelliq-labs/intentumdiff-core/issues/247) |
| `yaml` | 15 | 9 | 4 | 0 | 2 | 0 | 7 | candidate only | [intentumdiff-core#279](https://github.com/buchochelliq-labs/intentumdiff-core/issues/279), [intentumdiff-yaml-parser#4](https://github.com/buchochelliq-labs/intentumdiff-yaml-parser/issues/4), [intentumdiff-yaml-parser#5](https://github.com/buchochelliq-labs/intentumdiff-yaml-parser/issues/5) |
| `zig` | 9 | 8 | 0 | 0 | 1 | 0 | 0 | no external source | — |

## External-source coverage and remaining lanes

**103 declared external-derived cases cover 39 IDs.** By source admission: 99 unblocked candidates and 4 blocked cases in OCaml/R. Separately, by oracle scope: 99 source-judged cases and 4 partial-input probes. These are different partitions: all four admission-blocked OCaml/R cases are source-judged. There are 35 IDs without an imported external source, zero upstream-gold edit cases and zero fully externally admitted language IDs. A source pin, licence record or passing diagnostic probe does not establish complete benchmark admission.

IDs without imported external sources: `abap`, `adf`, `asm`, `assemblyscript`, `c`, `cmake`, `csharp`, `dart`, `databricks`, `databricks-workflow`, `dax`, `delphi`, `dockerfile`, `generic`, `gitignore`, `gomod`, `graphql`, `ini`, `kotlin`, `m`, `make`, `odin`, `plsql`, `postscript`, `proto`, `puppet`, `qsharp`, `sas`, `scala`, `sql`, `swift`, `tsql`, `vbnet`, `xml`, `zig`.

OCaml/R source-specific licence and attribution review remains open for: `dynamic/ocaml/external-derived-edit`, `dynamic/ocaml/external-derived-identity`, `dynamic/r/external-derived-edit`, `dynamic/r/external-derived-identity`.

Not assessed: Exact source-span accuracy beyond validity checks. Correspondence/mapping and explanation precision-recall. Complete construct coverage for any language. Sustained load, resource envelopes and concurrency limits. Cross-platform behavior and all compiler/runtime dialects.

Parent-child span containment is observational because node name spans can legitimately exclude descendants. Actual source bounds, ordering, UTF-8 boundaries, duplicate IDs and final group references were checked. Range validity does not establish exact-span accuracy. Source oracles are **declared**, not described as externally admitted.

## Bounded internal-error observations

Eight of the 75 bounded outcomes contain internal `literal enrich ... missing field id` fallback details across seven IDs. These are explicit, bounded observations requiring protocol/robustness follow-up under [core #280](https://github.com/buchochelliq-labs/intentumdiff-core/issues/280); they are not semantic correctness passes and are not added to the 129 confirmed-case denominator. Full protocol correctness has not been judged. The JSON retains each exact fallback, case ID and raw-artifact hash.

- `data_config.adf.incomplete`: ``literal enrich (new): literal enrich tree: missing field `id` at line 1 column 75``.
- `data_config.databricks-workflow.incomplete`: ``literal enrich (new): literal enrich tree: missing field `id` at line 1 column 43``.
- `data_config.databricks.incomplete`: ``literal enrich (new): literal enrich tree: missing field `id` at line 1 column 43``.
- `data_config.xml.incomplete`: ``literal enrich (new): literal enrich tree: missing field `id` at line 1 column 31``.
- `data_config.yaml.external_incomplete`: ``literal enrich (new): literal enrich tree: missing field `id` at line 1 column 31``.
- `data_config.yaml.incomplete`: ``literal enrich (new): literal enrich tree: missing field `id` at line 1 column 31``.
- `dynamic/powershell/incomplete`: ``literal enrich (old): literal enrich tree: missing field `id` at line 1 column 31``.
- `web-svelte-incomplete`: ``literal enrich (old): literal enrich tree: missing field `id` at line 1 column 42``.

## Manual explanation/evidence findings

- `abap-unicode-payload`: ABAP WRITE backtick string café -> cafe雪 is falsely explained as RENAME_VARIABLE CAF -> CAFE; there is no renamed binding. [intentumdiff-abap-parser#8](https://github.com/buchochelliq-labs/intentumdiff-abap-parser/issues/8).

- `dynamic/elixir/language-feature`: Elixir def -> defp changes public/private declaration semantics but is falsely explained as a variable rename. [intentumdiff-core#271](https://github.com/buchochelliq-labs/intentumdiff-core/issues/271).

- `web-tsx-external-module-quote-spelling`: TSX string-equivalence evidence reports absolute byte span [23,30], which slices a preamble comment instead of the string; the actual module literal is at [144,151]. The node coordinates are row 6, columns 23..30, so row offsets were omitted from the full-file evidence span. [intentumdiff-core#277](https://github.com/buchochelliq-labs/intentumdiff-core/issues/277).

## Prioritized repair queue

Start with shared core [#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278) (retain semantic hash deltas despite incomplete labels) and [#279](https://github.com/buchochelliq-labs/intentumdiff-core/issues/279) (preserve YAML sequence content/cardinality through suppression). Then address [#276](https://github.com/buchochelliq-labs/intentumdiff-core/issues/276) (complete multiline CSS/SCSS declaration values) and [#277](https://github.com/buchochelliq-labs/intentumdiff-core/issues/277) (full-file byte offsets for string-equivalence evidence), before working through the parser-specific issues.

Queue counts overlap; use the linked case IDs/issues as the work units. Reproduce each source fact, fix it in the owning Rust parser/core layer, rebuild and pin the component, then replay the exact regression and preservation controls.

- **P1 — Preserve meaningful values, bodies, operators and public contracts** (98 cases): [intentumdiff-abap-parser#7](https://github.com/buchochelliq-labs/intentumdiff-abap-parser/issues/7), [intentumdiff-adf-parser#7](https://github.com/buchochelliq-labs/intentumdiff-adf-parser/issues/7), [intentumdiff-asciidoc-parser#6](https://github.com/buchochelliq-labs/intentumdiff-asciidoc-parser/issues/6), [intentumdiff-assemblyscript-parser#5](https://github.com/buchochelliq-labs/intentumdiff-assemblyscript-parser/issues/5), [intentumdiff-astro-parser#17](https://github.com/buchochelliq-labs/intentumdiff-astro-parser/issues/17), [intentumdiff-bash-parser#4](https://github.com/buchochelliq-labs/intentumdiff-bash-parser/issues/4), [intentumdiff-core#243](https://github.com/buchochelliq-labs/intentumdiff-core/issues/243), [intentumdiff-core#247](https://github.com/buchochelliq-labs/intentumdiff-core/issues/247), [intentumdiff-core#254](https://github.com/buchochelliq-labs/intentumdiff-core/issues/254), [intentumdiff-core#275](https://github.com/buchochelliq-labs/intentumdiff-core/issues/275), [intentumdiff-core#276](https://github.com/buchochelliq-labs/intentumdiff-core/issues/276), [intentumdiff-core#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278), [intentumdiff-core#279](https://github.com/buchochelliq-labs/intentumdiff-core/issues/279), [intentumdiff-cpp-parser#4](https://github.com/buchochelliq-labs/intentumdiff-cpp-parser/issues/4), [intentumdiff-csharp-parser#4](https://github.com/buchochelliq-labs/intentumdiff-csharp-parser/issues/4), [intentumdiff-databricks-parser#7](https://github.com/buchochelliq-labs/intentumdiff-databricks-parser/issues/7), [intentumdiff-dax-parser#11](https://github.com/buchochelliq-labs/intentumdiff-dax-parser/issues/11), [intentumdiff-dockerfile-parser#5](https://github.com/buchochelliq-labs/intentumdiff-dockerfile-parser/issues/5), [intentumdiff-gitignore-parser#4](https://github.com/buchochelliq-labs/intentumdiff-gitignore-parser/issues/4), [intentumdiff-graphql-parser#8](https://github.com/buchochelliq-labs/intentumdiff-graphql-parser/issues/8), [intentumdiff-groovy-parser#4](https://github.com/buchochelliq-labs/intentumdiff-groovy-parser/issues/4), [intentumdiff-js-ts-parser#8](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/8), [intentumdiff-js-ts-parser#9](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/9), [intentumdiff-json-parser#4](https://github.com/buchochelliq-labs/intentumdiff-json-parser/issues/4), [intentumdiff-latex-parser#4](https://github.com/buchochelliq-labs/intentumdiff-latex-parser/issues/4), [intentumdiff-make-parser#4](https://github.com/buchochelliq-labs/intentumdiff-make-parser/issues/4), [intentumdiff-mdx-parser#7](https://github.com/buchochelliq-labs/intentumdiff-mdx-parser/issues/7), [intentumdiff-ocaml-parser#7](https://github.com/buchochelliq-labs/intentumdiff-ocaml-parser/issues/7), [intentumdiff-odin-parser#4](https://github.com/buchochelliq-labs/intentumdiff-odin-parser/issues/4), [intentumdiff-php-parser#7](https://github.com/buchochelliq-labs/intentumdiff-php-parser/issues/7), [intentumdiff-postscript-parser#6](https://github.com/buchochelliq-labs/intentumdiff-postscript-parser/issues/6), [intentumdiff-powershell-parser#8](https://github.com/buchochelliq-labs/intentumdiff-powershell-parser/issues/8), [intentumdiff-qsharp-parser#8](https://github.com/buchochelliq-labs/intentumdiff-qsharp-parser/issues/8), [intentumdiff-reasonml-parser#7](https://github.com/buchochelliq-labs/intentumdiff-reasonml-parser/issues/7), [intentumdiff-rust-parser#4](https://github.com/buchochelliq-labs/intentumdiff-rust-parser/issues/4), [intentumdiff-rust-parser#5](https://github.com/buchochelliq-labs/intentumdiff-rust-parser/issues/5), [intentumdiff-sas-parser#9](https://github.com/buchochelliq-labs/intentumdiff-sas-parser/issues/9), [intentumdiff-sql-parser#4](https://github.com/buchochelliq-labs/intentumdiff-sql-parser/issues/4), [intentumdiff-svelte-parser#7](https://github.com/buchochelliq-labs/intentumdiff-svelte-parser/issues/7), [intentumdiff-svelte-parser#8](https://github.com/buchochelliq-labs/intentumdiff-svelte-parser/issues/8), [intentumdiff-tsql-parser#4](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/4), [intentumdiff-vbnet-parser#6](https://github.com/buchochelliq-labs/intentumdiff-vbnet-parser/issues/6), [intentumdiff-vue-parser#6](https://github.com/buchochelliq-labs/intentumdiff-vue-parser/issues/6), [intentumdiff-wat-parser#4](https://github.com/buchochelliq-labs/intentumdiff-wat-parser/issues/4), [intentumdiff-yaml-parser#4](https://github.com/buchochelliq-labs/intentumdiff-yaml-parser/issues/4).

- **P1 — Repair invalid source ranges and final change-group references** (8 cases): [intentumdiff-core#243](https://github.com/buchochelliq-labs/intentumdiff-core/issues/243), [intentumdiff-core#254](https://github.com/buchochelliq-labs/intentumdiff-core/issues/254), [intentumdiff-databricks-parser#8](https://github.com/buchochelliq-labs/intentumdiff-databricks-parser/issues/8), [intentumdiff-generic-parser#4](https://github.com/buchochelliq-labs/intentumdiff-generic-parser/issues/4), [intentumdiff-plsql-parser#4](https://github.com/buchochelliq-labs/intentumdiff-plsql-parser/issues/4), [intentumdiff-tsql-parser#6](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/6).

- **P1 — Correct explanations and evidence that pass Boolean checks** (3 cases): [intentumdiff-abap-parser#8](https://github.com/buchochelliq-labs/intentumdiff-abap-parser/issues/8), [intentumdiff-core#271](https://github.com/buchochelliq-labs/intentumdiff-core/issues/271), [intentumdiff-core#277](https://github.com/buchochelliq-labs/intentumdiff-core/issues/277).

- **P2 — Remove false semantic alarms for justified preservation controls** (21 cases): [intentumdiff-asm-parser#4](https://github.com/buchochelliq-labs/intentumdiff-asm-parser/issues/4), [intentumdiff-core#270](https://github.com/buchochelliq-labs/intentumdiff-core/issues/270), [intentumdiff-core#272](https://github.com/buchochelliq-labs/intentumdiff-core/issues/272), [intentumdiff-core#273](https://github.com/buchochelliq-labs/intentumdiff-core/issues/273), [intentumdiff-core#274](https://github.com/buchochelliq-labs/intentumdiff-core/issues/274), [intentumdiff-core#275](https://github.com/buchochelliq-labs/intentumdiff-core/issues/275), [intentumdiff-core#276](https://github.com/buchochelliq-labs/intentumdiff-core/issues/276), [intentumdiff-dockerfile-parser#4](https://github.com/buchochelliq-labs/intentumdiff-dockerfile-parser/issues/4), [intentumdiff-kotlin-parser#5](https://github.com/buchochelliq-labs/intentumdiff-kotlin-parser/issues/5), [intentumdiff-ocaml-parser#8](https://github.com/buchochelliq-labs/intentumdiff-ocaml-parser/issues/8), [intentumdiff-plsql-parser#5](https://github.com/buchochelliq-labs/intentumdiff-plsql-parser/issues/5), [intentumdiff-postscript-parser#7](https://github.com/buchochelliq-labs/intentumdiff-postscript-parser/issues/7), [intentumdiff-reasonml-parser#8](https://github.com/buchochelliq-labs/intentumdiff-reasonml-parser/issues/8), [intentumdiff-swift-parser#5](https://github.com/buchochelliq-labs/intentumdiff-swift-parser/issues/5), [intentumdiff-tsql-parser#5](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/5), [intentumdiff-vue-parser#6](https://github.com/buchochelliq-labs/intentumdiff-vue-parser/issues/6), [intentumdiff-yaml-parser#5](https://github.com/buchochelliq-labs/intentumdiff-yaml-parser/issues/5).

## Newly filed issues

- [buchochelliq-labs/intentumdiff-abap-parser#8](https://github.com/buchochelliq-labs/intentumdiff-abap-parser/issues/8) — [0.0.3][P1] Preserve backtick string literals instead of inventing variable renames (1 cases).

- [buchochelliq-labs/intentumdiff-asm-parser#4](https://github.com/buchochelliq-labs/intentumdiff-asm-parser/issues/4) — [0.0.3][P2] Do not turn ordinary comments or blank lines into instructions (2 cases).

- [buchochelliq-labs/intentumdiff-assemblyscript-parser#5](https://github.com/buchochelliq-labs/intentumdiff-assemblyscript-parser/issues/5) — [0.0.3][P1] Retain returned string payloads in semantic hashes (1 cases).

- [buchochelliq-labs/intentumdiff-astro-parser#17](https://github.com/buchochelliq-labs/intentumdiff-astro-parser/issues/17) — [0.0.3][P1] Retain static Astro template text in semantic identity (3 cases).

- [buchochelliq-labs/intentumdiff-bash-parser#4](https://github.com/buchochelliq-labs/intentumdiff-bash-parser/issues/4) — [0.0.3][P1] Retain Bash test-command comparison operators (1 cases).

- [buchochelliq-labs/intentumdiff-core#270](https://github.com/buchochelliq-labs/intentumdiff-core/issues/270) — [0.0.3][P2] Do not promote Perl assignment-label spacing into a semantic change (1 cases).

- [buchochelliq-labs/intentumdiff-core#271](https://github.com/buchochelliq-labs/intentumdiff-core/issues/271) — [0.0.3][P1] Do not classify Elixir def→defp visibility changes as variable renames (1 cases).

- [buchochelliq-labs/intentumdiff-core#272](https://github.com/buchochelliq-labs/intentumdiff-core/issues/272) — [0.0.3][P2] Recognize equivalent embedded JavaScript declaration strings in Astro and Svelte (2 cases).

- [buchochelliq-labs/intentumdiff-core#273](https://github.com/buchochelliq-labs/intentumdiff-core/issues/273) — [0.0.3][P2] Exclude CSS comments from synthetic property names (1 cases).

- [buchochelliq-labs/intentumdiff-core#274](https://github.com/buchochelliq-labs/intentumdiff-core/issues/274) — [0.0.3][P2] Recognize equivalent CommonMark code-block forms without inventing semantic changes (1 cases).

- [buchochelliq-labs/intentumdiff-core#275](https://github.com/buchochelliq-labs/intentumdiff-core/issues/275) — [0.0.3][P1] Preserve Markdown/MDX ATX heading levels and optional closing hashes (4 cases).

- [buchochelliq-labs/intentumdiff-core#276](https://github.com/buchochelliq-labs/intentumdiff-core/issues/276) — [0.0.3][P1] Retain complete multiline CSS/SCSS declaration values in source enrichment (2 cases).

- [buchochelliq-labs/intentumdiff-core#277](https://github.com/buchochelliq-labs/intentumdiff-core/issues/277) — [0.0.3][P2] Anchor string-equivalence source evidence to full-file byte offsets (1 cases).

- [buchochelliq-labs/intentumdiff-core#278](https://github.com/buchochelliq-labs/intentumdiff-core/issues/278) — [0.0.3][P1] Preserve semantic hash deltas when labels omit Make, Proto, Puppet and TOML payloads (5 cases).

- [buchochelliq-labs/intentumdiff-core#279](https://github.com/buchochelliq-labs/intentumdiff-core/issues/279) — [0.0.3][P1] Preserve YAML sequence content and cardinality through representation suppression (1 cases).

- [buchochelliq-labs/intentumdiff-csharp-parser#4](https://github.com/buchochelliq-labs/intentumdiff-csharp-parser/issues/4) — [0.0.3][P1] Preserve readonly field modifiers in semantic projection (1 cases).

- [buchochelliq-labs/intentumdiff-dockerfile-parser#5](https://github.com/buchochelliq-labs/intentumdiff-dockerfile-parser/issues/5) — [0.0.3][P1] Retain exec-form CMD string arguments in semantic output (1 cases).

- [buchochelliq-labs/intentumdiff-groovy-parser#4](https://github.com/buchochelliq-labs/intentumdiff-groovy-parser/issues/4) — [0.0.3][P1] Retain Groovy numeric return values and arithmetic arguments (2 cases).

- [buchochelliq-labs/intentumdiff-js-ts-parser#9](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/9) — [0.0.3][P1] Retain TSX boolean JSX prop expression values (1 cases).

- [buchochelliq-labs/intentumdiff-kotlin-parser#5](https://github.com/buchochelliq-labs/intentumdiff-kotlin-parser/issues/5) — [0.0.3][P2] Keep ordinary line comments out of executable expression nodes (1 cases).

- [buchochelliq-labs/intentumdiff-ocaml-parser#8](https://github.com/buchochelliq-labs/intentumdiff-ocaml-parser/issues/8) — [0.0.3][P2] Treat insignificant OCaml let-RHS spacing as preservation (1 cases).

- [buchochelliq-labs/intentumdiff-php-parser#7](https://github.com/buchochelliq-labs/intentumdiff-php-parser/issues/7) — [0.0.3][P1] Retain PHP double-quoted echo string values in semantic evidence (1 cases).

- [buchochelliq-labs/intentumdiff-plsql-parser#5](https://github.com/buchochelliq-labs/intentumdiff-plsql-parser/issues/5) — [0.0.3][P1] Skip leading PL/SQL comments when finding routine block signatures (1 cases).

- [buchochelliq-labs/intentumdiff-postscript-parser#7](https://github.com/buchochelliq-labs/intentumdiff-postscript-parser/issues/7) — [0.0.3][P2] Treat inter-token PostScript spacing as preservation (1 cases).

- [buchochelliq-labs/intentumdiff-powershell-parser#8](https://github.com/buchochelliq-labs/intentumdiff-powershell-parser/issues/8) — [0.0.3][P1] Retain PowerShell return literals and comparison operators (2 cases).

- [buchochelliq-labs/intentumdiff-reasonml-parser#8](https://github.com/buchochelliq-labs/intentumdiff-reasonml-parser/issues/8) — [0.0.3][P2] Treat insignificant ReasonML let-RHS spacing as preservation (1 cases).

- [buchochelliq-labs/intentumdiff-rust-parser#5](https://github.com/buchochelliq-labs/intentumdiff-rust-parser/issues/5) — [0.0.3][P1] Preserve reference parameter mutability in semantic projection (1 cases).

- [buchochelliq-labs/intentumdiff-svelte-parser#8](https://github.com/buchochelliq-labs/intentumdiff-svelte-parser/issues/8) — [0.0.3][P1] Retain static Svelte template text content (2 cases).

- [buchochelliq-labs/intentumdiff-swift-parser#5](https://github.com/buchochelliq-labs/intentumdiff-swift-parser/issues/5) — [0.0.3][P2] Keep ordinary line comments out of executable expression nodes (1 cases).

- [buchochelliq-labs/intentumdiff-tsql-parser#5](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/5) — [0.0.3][P1] Retain T-SQL statements after leading line comments (1 cases).

- [buchochelliq-labs/intentumdiff-tsql-parser#6](https://github.com/buchochelliq-labs/intentumdiff-tsql-parser/issues/6) — [0.0.3][P2] Bound T-SQL wrapper EOF positions to the actual source rows (1 cases).

- [buchochelliq-labs/intentumdiff-yaml-parser#5](https://github.com/buchochelliq-labs/intentumdiff-yaml-parser/issues/5) — [0.0.3][P1] Keep leading YAML comments out of semantic root labels (2 cases).

## Evidence files

- [Independent review](2026-11-08-language-assessment-review.md): source, result, issue-map and reporting checks, with the remaining assessment limits.

- [Enriched 74-language assessment](2026-11-08-language-assessment.json): original run identity and complete parser manifest, hash-pinned fixtures, all case outcomes, issue links, manual findings and coverage limits.

- [Compact confirmed-case evidence](2026-11-08-language-assessment-findings.json): 129 cases with review flags, changes/groups, relevant invariance evidence, errors and raw-artifact hashes; complete parse trees are excluded.

- [Assessment guide](../LANGUAGE_ASSESSMENT.md).

No percentage accuracy, cross-platform guarantee, full construct coverage or sustained-load certification is claimed.
