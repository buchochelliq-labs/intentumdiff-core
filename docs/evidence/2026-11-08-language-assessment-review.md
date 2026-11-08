# Independent review: all-language diagnostic assessment

The assessment executes a useful bounded diagnostic suite for every one of the 74 inventory IDs. It does **not** establish full language correctness, external benchmark admission for all languages, or explanation/correspondence accuracy. I independently reviewed all 760 case definitions and their source-based observation domains, all final failed/invalid rows and their actual outputs, the production runner, source acquisition records, and selected passing explanations. I found passing verdicts with wrong facts, as well as real classification and source-bound failures.

This review makes no engine changes and sends no external comments or issues. The parent owns issue creation and the final scorecard. References to candidate groups below describe confirmed behavior; suggested first-loss ownership remains subject to component isolation where stated.

## Candidate and execution identity

- Production Rust C ABI calls: `live_diff_contents` and standalone `parse_to_tree`; installed Python only transports these calls.
- Native library SHA256: `4bd34bb25a463d68f89a5aa536a4fd694b43261969972c75b3e73332e1bf94bd`.
- Registry pin: `8911328ede24630ad9ea6a33da8ed010f066d3b1`; all 68 unique staged component hashes were independently checked against the manifest. Canonical C/C++ component SHA256: `447196da1ea6f2ee2f9adb10fe02d14d6da3a85d8eee06059a01aaaeb20eb483`.
- Every per-ID worker sets `strict_plugins:true` and an explicit matching `language_hint`. Actual review and parse returned IDs are checked. Shared components/aliases remain separate rows; Python intentionally uses the first-party native tree-sitter route after component probing, and generic review uses its native text route while its standalone component parse is also inspected. These are actual implemented routes, not proof that every operation used Wasm.
- Current result artifacts: `build/language-assessment/final-reviewed/assessment.json` and `build/language-assessment/final-reviewed/raw/<language_id>.json`. Fixture source strings, expected values, component/native fingerprints and raw outputs are retained; build artifacts are local evidence until the parent publishes durable links.
- Reviewed run UTC: `2026-11-08T15:38:22.359296+00:00`; final assessment JSON SHA256: `5e1ae51d7c399b16f7a092cb4234dd730bca9e496d3bf7919518dbe247f9b809`.

## Result accounting and freshness

The reviewed raw set contains 760 rows, 74 IDs and 760 unique case IDs. Status counts: bounded_explicit_fallback=32, bounded_incomplete_observation=43, failed=123, invalid_result=3, passed=559.

All 760 current fixture old/new SHA256 values and expected objects match the final raw rows exactly; no case is missing or duplicated. All four fixture-file hashes match. Per-ID summary counts, strict routing options, actual returned language IDs and native fingerprints agree with raw evidence. The previous stale Gitignore comment row was replaced and rerun; all other 759 normalized review outputs match the previously reviewed run.

There are no worker errors, runtime exceptions, routing mismatches, recorded parser-error arrays, or normalized-output determinism failures in this run. This is a result for these bounded inputs, not a crash-free or performance certification. Thirty-two partial/boundary cases return explicit fallback envelopes: 24 report incomplete syntax and eight expose an internal `literal enrich ... missing field id` diagnostic. They are bounded fallback observations, not semantic successes. Forty-three partial cases return bounded semantic observations without a verdict oracle; three partial cases return actual out-of-source tree endpoints and fail validity.

Six rows have actual out-of-source endpoints: Databricks and its alias EOF identities; PL/SQL incomplete and no-final-LF identity; T-SQL incomplete; generic incomplete. Two rows have stale change-group references: CSS keyword case and Python str-to-bytes (the latter also fails classification). These reference failures must be separated from source-range failures. Parent/child range containment alone is uncontracted and non-gating in this assessment.

The suite has 682 source-judged flag expectations (417 meaningful, 265 preservation) and 78 unconstrained partial/boundary cases. A boolean pass says only that the asserted flag/empty-array expectations and bounded validity checks passed. It does not validate every node kind, typed fact, correspondence, grouping, description, or exact span.

### Unresolved internal-fallback robustness investigation

Retain these eight observations under the parent repair tracker [core#280](https://github.com/buchochelliq-labs/intentumdiff-core/issues/280): `data_config.adf.incomplete`, `data_config.databricks.incomplete`, `data_config.databricks-workflow.incomplete`, `dynamic/powershell/incomplete`, `web-svelte-incomplete`, `data_config.xml.incomplete`, `data_config.yaml.incomplete`, and `data_config.yaml.external_incomplete`. They span seven IDs. Their raw envelopes report `literal enrich ... missing field id`; this is weaker diagnostic evidence than an intentional source-syntax rejection. They satisfy the current bounded explicit-handling oracle, but are not certified correct invalid-input diagnostics or counted as semantic passes.

Inspect the original component error envelopes and host enrichment/translation path before deciding whether information is lost or a protocol requirement is violated. SDK#24 is related compliance/error-envelope work, not an exact proven duplicate. No new confirmed correctness ticket follows from the message alone. The 129-case defect map covers adjudicated confirmed failures/evidence defects; it does not imply these robustness observations are perfect or assessed correct.

## Evidence classes and admission limits

| Class | Cases |
|---|---:|
| `handwritten_regression` | 488 |
| `repo_fixture` | 169 |
| `reviewed_external_derived` | 103 |

There are 732 unique old/new byte pairs versus 760 per-ID pairs. Reused and alias cases must be deduplicated for a portfolio source total; per-ID routing outcomes must remain distinct. The four family sizes are compiled 220/20 IDs, dynamic 189/17, web 136/14, data/config 215/23.

The 103 external-derived probes reach 39 IDs, with 99 source-judged pairs and four partial-input probes. They are small author-reviewed adaptations, **zero upstream gold**, and not original published benchmark metrics. All external suites still need full adapter/environment/holdout/admission work before broad claims. OCaml and R's four external-derived records additionally carry explicit source-specific attribution admission blocks. Licence text retention and byte identity are not a legal opinion or proof that every suite condition has been satisfied.

No current-suite external-derived case exists for these 35 IDs: `abap`, `adf`, `asm`, `assemblyscript`, `c`, `cmake`, `csharp`, `dart`, `databricks`, `databricks-workflow`, `dax`, `delphi`, `dockerfile`, `generic`, `gitignore`, `gomod`, `graphql`, `ini`, `kotlin`, `m`, `make`, `odin`, `plsql`, `postscript`, `proto`, `puppet`, `qsharp`, `sas`, `scala`, `sql`, `swift`, `tsql`, `vbnet`, `xml`, `zig`. Earlier pilot evidence is separate and must not silently fill these current-run denominators.

## Independent source/provenance checks

- All 59 distinct repository-source records match both declared SHA256 and exact `git show` bytes at `7ae58e9237e0be07ddf919b6d7a7cbc8a1333983`.
- Independently refetched every retained original GitHub source/licence/notice/expected-output acquisition used by dynamic and data/config (47 records), all 24 web acquisitions, and both WebAssembly source/licence acquisitions at their exact immutable pins. For complete files, reconstructed Git blob SHA1 agrees with the service's pinned blob SHA; local SHA256/byte counts also agree. CommonMark's explicitly declared lines-1..800 excerpt was compared to that exact fetched range. Total: 73 remote-original records, not 73 independent benchmark tasks.
- All 13 selected HumanEvalPack JSON records were independently dictionary-compared to their original pinned Parquet rows using PyArrow. Each of the four retained Parquet copies matches its earlier immutable source copy byte-for-byte. This verifies source/task identity, not all upstream runtime assertions.
- Independently executed 25 ordinary paired Bash/JavaScript/Perl/Python source witnesses, including observed string/literal/operator/spacing/comment controls and a JavaScript long-string tail. Their observed outputs agree with the declared domains. This is not execution of all languages or proof of arbitrary program equivalence.
- Family authors additionally report four C++ repairs tested with retained upstream assertion programs (eight builds, old assertions fail/new pass), decoded-format witnesses, GNU Make assignment-flavour execution, and Git ignore matching witnesses. Those are author-produced reference evidence, distinct from the independent checks above.

No invented external-gold labels were found. Original source bytes remain separate from derived probe strings and expected IntentumDiff flags. Fixed decoder/runtime/dialect assumptions in oracle prose matter: public type/linkage/visibility edits observe the callable/schema contract, not necessarily a runtime difference for the tiny supplied main program.

## Oracle and harness corrections required during review

The review identified and authors corrected malformed claimed complete INI inputs by adding `[DEFAULT]`; stale T-SQL alias use after a rename; stale HCL resource references after a rename; a wrong HCL claim that number-to-string decoding necessarily rejects; inaccurate Bash witness prose; and blanket claims that partial inputs must be rejected. Exact identity controls now leave `is_style_only` unconstrained. Ordinary preservation controls and equivalent syntax often leave representation/style flags unconstrained while asserting the declared semantic observation.

Gitignore's original comment preservation control was withdrawn because the product's review-content treatment of comments needs a distinct contract from Git's ignore matching. The replacement is an unambiguous blank-line control. This removes comment classification from the assessed claims; it does not prove the original engine comment verdict correct. YAML comment failure remains concrete: a root label becomes the comment and invents a decoded-value change, as the actual typed-value profile and emitted facts show.

The runner initially had false greens/false failures which the parent repaired following review: weak strict-plugin configuration; validating only standalone trees rather than returned diff nodes; treating lost timeout rows as executed; unconstrained incomplete cases bypassing result validity; external path resolution; blanket parent containment; valid explicit fallback envelopes rejected as JSON errors; and fuel/latency measurement variation mistaken for semantic nondeterminism. The SQL long-literal case was independently rerun three times: semantic output agreed, only fuel-hotspot measurement values varied. These measurements are normalized only for determinism comparison and remain in raw output.

Latest independent tooling verification: `/workspace/venv/bin/python -m pytest -q tests/tooling/test_language_assessment.py` — **11 passed**. These adversarial tests cover the checker/accounting, not the language engine's correctness. No broad repeated engine test claim is made from them.

## Confirmed explanation/evidence defects in passing rows

1. `dynamic/elixir/language-feature`: `def answer` to `defp answer` changes public/private callable visibility. Actual final change is `REFACTORING/RENAME_VARIABLE`, confidence 1, description `Rename variable 'def' -> 'defp'`. The flags pass for the wrong source fact. Existing core#245 is related; the exact keyword/visibility witness is additional evidence and must be logged.
2. `abap-unicode-payload`: emitted text changes from backtick-quoted `café` to `cafe雪`; output instead claims `Rename variable 'CAF' -> 'CAFE'`, confidence 1. Parsed identifier spans exclude the non-ASCII literal payload and the correct literal nature. The source-fact defect is distinct from a source endpoint merely being in bounds. ABAP parser#7 is related, not this exact quoted literal case.
3. `web-tsx-external-module-quote-spelling`: decoded module name `react` preservation is correctly recognized, but `evidence_depth:source_span` publishes `[23,30]` for both full-source spans. Those bytes are `\n// @js`; actual CRLF-source module literal bytes are `[144,151]`. The node itself has correct line 6 columns 23..30; host equivalence metadata confuses line-local byte columns with full-source offsets. Core#254 is related. The source-node range checker does not cover this metadata domain.

Duplicate occurrence correspondence in the AsciiDoc external paragraph and MDX heading `RENAME_SYMBOL` taxonomy are recorded as observations, not inflated confirmed failures. Likewise generic function-identifier rename descriptions do not establish explanation precision. No explanation-fact precision/recall rate is claimed.

## Independently confirmed new behavior groups

The per-case appendix below distinguishes exact existing defects from candidate groups. Existing family acquisition issues #262–#268 are requirements, not automatic bug duplicates. Core#252/#253/#254 and SDK#25 are related investigations unless the exact existing ticket covers the concrete trigger. Grouping is by observed mechanism, not by number of failed assertions.

- **Parser payload/token loss:** ASM comments/blank lines create empty instructions; AssemblyScript returned string payload disappears; C# readonly field modifier disappears; Rust reference mutability disappears; Groovy return/call arithmetic disappears; PHP echo Unicode literal disappears; PowerShell return literals and comparison operators disappear; Bash test comparison operators disappear; Dockerfile exec-form CMD payload disappears. Actual trees/hash/label evidence confirms these failures. Existing Rust async/unsafe and C/C++ qualifier tickets are reused for their existing triggers, not duplicated.
- **Host comparison/equivalence:** normalized type/label/child projection ignores differing semantic hashes (Make, Proto, Puppet, plus TOML dotted table-name witness); XML/HTML significant-space losses reuse existing issues. YAML sequence insertion is completely present in the tree but is erased by `presentation.suppress_yaml_representation_equivalent_modification` using generic/positional label evidence. This YAML case needs its own coverage-aware suppression fix.
- **Preservation false alarms:** OCaml/ReasonML raw RHS spacing, Perl raw assignment label spacing, PostScript token spacing; ordinary Kotlin/Swift comments emitted as expressions; PL/SQL comment mis-splits the block/signature; T-SQL leading comment loses the SELECT node; YAML leading comment changes the root label; CSS comment becomes part of a synthetic property name; embedded Astro/Svelte equivalent JS quote spelling is reported meaningful.
- **Markup/document payload:** Astro and Svelte static text; TSX Boolean prop values; Markdown/MDX opening/closing ATX heading semantics; CommonMark equivalent indented/fenced code-block forms. The HTML rendering/domain or static component contract is stated independently. Vue inline/style formatting failures reuse Vue#6, and LaTeX options reuse LaTeX#4.
- **Multiline style evidence:** official Sass escaped-newline string continuation is omitted from synthetic property values. A changed continuation is falsely suppressed and equivalent one-line spelling falsely meaningful. This is core source-enrichment evidence loss, not a span-containment issue.
- **Result integrity:** CSS canonical color case produces correct no-change flags but appends `invariance_input.raw_change_indices:[0]` after final `changes` became empty. Python bytes has the same stale-reference symptom and an existing core#243 classification failure. The documented index-space contract requires final reconciliation or honestly empty ownership, even for specialized spaces.
- **Source bounds:** T-SQL incomplete root ends at nonexistent `(5,0)` and is a new EOF validity witness. Databricks, generic and PL/SQL endpoint defects map exact existing tickets. Kotlin/Swift root-vs-child containment is left unconfirmed and non-gating; no new confirmed ticket should be based only on that convention.

## All 74 per-ID outcomes

`P/F` counts constrained cases passing/failing all checks; `C` counts flag mismatches; `R` counts actual source-range errors; `G` counts stale final change references; `B` counts bounded unconstrained cases and explicit fallbacks; `I` counts unconstrained cases with invalid results. `E` is acquired external-derived cases, not admitted gold. Failures can overlap C/R/G; these are case counts, not three disjoint totals.

| ID | Cases | P | F | C | R | G | B | I | E |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| abap | 10 | 7 | 2 | 2 | 0 | 0 | 1 | 0 | 0 |
| adf | 8 | 3 | 4 | 4 | 0 | 0 | 1 | 0 | 0 |
| asciidoc | 10 | 7 | 2 | 2 | 0 | 0 | 1 | 0 | 2 |
| asm | 9 | 6 | 2 | 2 | 0 | 0 | 1 | 0 | 0 |
| assemblyscript | 10 | 8 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| astro | 10 | 5 | 4 | 4 | 0 | 0 | 1 | 0 | 2 |
| bash | 11 | 9 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| c | 13 | 10 | 2 | 2 | 0 | 0 | 1 | 0 | 0 |
| clojure | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| cmake | 7 | 6 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| cpp | 18 | 15 | 2 | 2 | 0 | 0 | 1 | 0 | 4 |
| csharp | 10 | 8 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| css | 10 | 7 | 2 | 1 | 0 | 1 | 1 | 0 | 2 |
| dart | 9 | 8 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| databricks-workflow | 10 | 5 | 4 | 3 | 1 | 0 | 1 | 0 | 0 |
| databricks | 10 | 5 | 4 | 3 | 1 | 0 | 1 | 0 | 0 |
| dax | 7 | 6 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| delphi | 9 | 8 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| dockerfile | 8 | 5 | 2 | 2 | 0 | 0 | 1 | 0 | 0 |
| elixir | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| generic | 8 | 7 | 0 | 0 | 1 | 0 | 0 | 1 | 0 |
| gitignore | 10 | 8 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| go | 12 | 11 | 0 | 0 | 0 | 0 | 1 | 0 | 3 |
| gomod | 7 | 6 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| graphql | 10 | 5 | 4 | 4 | 0 | 0 | 1 | 0 | 0 |
| groovy | 11 | 8 | 2 | 2 | 0 | 0 | 1 | 0 | 2 |
| haskell | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| hcl | 14 | 12 | 0 | 0 | 0 | 0 | 2 | 0 | 7 |
| html | 10 | 8 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| ini | 7 | 6 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| java | 13 | 12 | 0 | 0 | 0 | 0 | 1 | 0 | 3 |
| javascript | 12 | 10 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| json | 15 | 12 | 1 | 1 | 0 | 0 | 2 | 0 | 7 |
| kotlin | 9 | 7 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| latex | 10 | 4 | 5 | 5 | 0 | 0 | 1 | 0 | 2 |
| lua | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| m | 9 | 5 | 3 | 3 | 0 | 0 | 1 | 0 | 0 |
| make | 8 | 4 | 3 | 3 | 0 | 0 | 1 | 0 | 0 |
| markdown | 10 | 5 | 4 | 4 | 0 | 0 | 1 | 0 | 2 |
| mdx | 10 | 4 | 5 | 5 | 0 | 0 | 1 | 0 | 2 |
| ocaml | 11 | 8 | 2 | 2 | 0 | 0 | 1 | 0 | 2 |
| odin | 9 | 7 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| perl | 11 | 9 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| php | 11 | 9 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| plsql | 8 | 5 | 2 | 1 | 2 | 0 | 0 | 1 | 0 |
| po | 10 | 9 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| postscript | 8 | 4 | 3 | 3 | 0 | 0 | 1 | 0 | 0 |
| powershell | 11 | 8 | 2 | 2 | 0 | 0 | 1 | 0 | 2 |
| proto | 7 | 5 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| puppet | 7 | 5 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| python | 11 | 9 | 1 | 1 | 0 | 1 | 1 | 0 | 2 |
| qsharp | 10 | 4 | 5 | 5 | 0 | 0 | 1 | 0 | 0 |
| r | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| reasonml | 11 | 8 | 2 | 2 | 0 | 0 | 1 | 0 | 2 |
| ruby | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| rust | 15 | 11 | 3 | 3 | 0 | 0 | 1 | 0 | 3 |
| sas | 10 | 6 | 3 | 3 | 0 | 0 | 1 | 0 | 0 |
| scala | 9 | 8 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |
| scss | 10 | 7 | 2 | 2 | 0 | 0 | 1 | 0 | 2 |
| sql | 8 | 6 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| squirrel | 11 | 10 | 0 | 0 | 0 | 0 | 1 | 0 | 2 |
| svelte | 10 | 4 | 5 | 5 | 0 | 0 | 1 | 0 | 2 |
| swift | 9 | 7 | 1 | 1 | 0 | 0 | 1 | 0 | 0 |
| toml | 14 | 11 | 1 | 1 | 0 | 0 | 2 | 0 | 7 |
| tsql | 8 | 5 | 2 | 2 | 1 | 0 | 0 | 1 | 0 |
| tsx | 10 | 8 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| typescript | 12 | 10 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| vbnet | 11 | 8 | 2 | 2 | 0 | 0 | 1 | 0 | 0 |
| vue | 10 | 2 | 7 | 7 | 0 | 0 | 1 | 0 | 2 |
| wast | 13 | 11 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| wat | 13 | 11 | 1 | 1 | 0 | 0 | 1 | 0 | 2 |
| xml | 8 | 5 | 2 | 2 | 0 | 0 | 1 | 0 | 0 |
| yaml | 15 | 9 | 4 | 4 | 0 | 0 | 2 | 0 | 7 |
| zig | 9 | 8 | 0 | 0 | 0 | 0 | 1 | 0 | 0 |

## Complete failed/invalid-row review ledger

Every failed/invalid row below was read against its actual source delta, oracle domain and returned result. Source strings and exact node evidence are in the cited per-ID raw file and the case definition; long payloads are not duplicated here. Group ownership is a ticket-triage conclusion, not an engine repair performed by this review.

| Case ID | Confirmed domain / disposition |
|---|---|
| `abap-data-initializer` | classification: intentumdiff-abap-parser#7 |
| `abap-declaration-api` | classification: intentumdiff-abap-parser#7 |
| `asm-layout` | classification: asm-trivia-creates-empty-instruction |
| `asm-ordinary-comment` | classification: asm-trivia-creates-empty-instruction |
| `assemblyscript-unicode-payload` | classification: assemblyscript-string-payload-erased |
| `c-const-object` | classification: intentumdiff-cpp-parser#4 |
| `c-storage-linkage` | classification: intentumdiff-cpp-parser#4 |
| `cpp-const-member` | classification: intentumdiff-cpp-parser#4 |
| `cpp-storage-linkage` | classification: intentumdiff-cpp-parser#4 |
| `csharp-readonly-field` | classification: csharp-readonly-field-erased |
| `data_config.adf.known_retry_policy` | classification: intentumdiff-adf-parser#7 (corrected WebActivity source) |
| `data_config.adf.meaningful_1_value` | classification: intentumdiff-adf-parser#7 |
| `data_config.adf.meaningful_3_body` | classification: intentumdiff-adf-parser#7 |
| `data_config.adf.unicode_value` | classification: intentumdiff-adf-parser#7 |
| `data_config.databricks-workflow.known_base_parameter` | classification: intentumdiff-databricks-parser#7 |
| `data_config.databricks-workflow.known_eof_identity` | source bounds: intentumdiff-databricks-parser#8 |
| `data_config.databricks-workflow.known_notebook_payload` | classification: intentumdiff-databricks-parser#7 |
| `data_config.databricks-workflow.meaningful_2_configuration` | classification: intentumdiff-databricks-parser#7 |
| `data_config.databricks.known_base_parameter` | classification: intentumdiff-databricks-parser#7 |
| `data_config.databricks.known_eof_identity` | source bounds: intentumdiff-databricks-parser#8 |
| `data_config.databricks.known_notebook_payload` | classification: intentumdiff-databricks-parser#7 |
| `data_config.databricks.meaningful_2_configuration` | classification: intentumdiff-databricks-parser#7 |
| `data_config.dockerfile.known_crlf` | classification: intentumdiff-dockerfile-parser#4 |
| `data_config.dockerfile.meaningful_3_body` | classification: data_config.dockerfile.exec_cmd_payload |
| `data_config.gitignore.known_leading_space` | classification: intentumdiff-gitignore-parser#4 |
| `data_config.graphql.known_hash_in_string` | classification: intentumdiff-graphql-parser#8 |
| `data_config.graphql.known_inline_field_type` | classification: intentumdiff-graphql-parser#8 |
| `data_config.graphql.known_operation_kind` | classification: intentumdiff-graphql-parser#8 |
| `data_config.graphql.meaningful_2_type` | classification: intentumdiff-graphql-parser#8 |
| `data_config.json.known_long_literal_suffix` | classification: intentumdiff-json-parser#4 |
| `data_config.m.known_inline_let` | classification: intentumdiff-dax-parser#11 |
| `data_config.m.known_multiline_value` | classification: intentumdiff-dax-parser#11 |
| `data_config.m.unicode_value` | classification: intentumdiff-dax-parser#11 |
| `data_config.make.known_assignment_flavor` | classification: intentumdiff-make-parser#4 |
| `data_config.make.meaningful_1_value` | classification: data_config.core.normalized_projection_discards_hash_delta |
| `data_config.make.unicode_value` | classification: data_config.core.normalized_projection_discards_hash_delta |
| `data_config.plsql.incomplete` | source bounds: intentumdiff-plsql-parser#4 |
| `data_config.plsql.known_no_final_newline` | source bounds: intentumdiff-plsql-parser#4 |
| `data_config.plsql.safe_preservation` | classification: data_config.plsql.leading_comment_block_split |
| `data_config.proto.unicode_value` | classification: data_config.core.normalized_projection_discards_hash_delta |
| `data_config.puppet.meaningful_2_type` | classification: data_config.core.normalized_projection_discards_hash_delta |
| `data_config.sas.known_inline_statement` | classification: intentumdiff-sas-parser#9 |
| `data_config.sas.known_macro_default` | classification: intentumdiff-sas-parser#9 |
| `data_config.sas.known_proc_option` | classification: intentumdiff-sas-parser#9 |
| `data_config.sql.known_long_literal_suffix` | classification: intentumdiff-sql-parser#4 |
| `data_config.toml.external_meaningful_3_configuration` | classification: normalized_projection_discards_hash_delta; TOML dotted-key omission |
| `data_config.tsql.incomplete` | source bounds: data_config.tsql.eof_root_outside_source |
| `data_config.tsql.meaningful_3_operator` | classification: intentumdiff-tsql-parser#4 |
| `data_config.tsql.safe_preservation` | classification: data_config.tsql.leading_comment_select_loss |
| `data_config.xml.known_text_whitespace` | classification: intentumdiff-core#247 |
| `data_config.xml.meaningful_3_significant_whitespace` | classification: intentumdiff-core#247 |
| `data_config.yaml.external_meaningful_3_body` | classification: yaml-representation-equivalence-erases-sequence-insertion |
| `data_config.yaml.external_safe_preservation` | classification: data_config.yaml.leading_comment_root_label |
| `data_config.yaml.known_long_literal_suffix` | classification: intentumdiff-yaml-parser#4 |
| `data_config.yaml.safe_preservation` | classification: data_config.yaml.leading_comment_root_label |
| `dynamic/bash/external-derived-edit` | classification: bash-comparison-operator-loss |
| `dynamic/groovy/literal` | classification: groovy-numeric-payload-loss |
| `dynamic/groovy/operator` | classification: groovy-numeric-payload-loss |
| `dynamic/javascript/literal-tail-after-4096` | classification: intentumdiff-js-ts-parser#8 |
| `dynamic/ocaml/language-feature` | classification: intentumdiff-ocaml-parser#7 |
| `dynamic/ocaml/spacing` | classification: ocaml-rhs-whitespace-false-positive |
| `dynamic/perl/spacing` | classification: perl-assignment-whitespace-false-positive |
| `dynamic/php/unicode-value` | classification: php-echo-string-payload-loss |
| `dynamic/powershell/external-derived-edit` | classification: powershell-semantic-expression-payload-loss |
| `dynamic/powershell/literal` | classification: powershell-semantic-expression-payload-loss |
| `dynamic/python/language-feature` | classification, group reference: intentumdiff-core#243; final-group-index-integrity |
| `dynamic/reasonml/language-feature` | classification: intentumdiff-reasonml-parser#7 |
| `dynamic/reasonml/spacing` | classification: reasonml-rhs-whitespace-false-positive |
| `dynamic/typescript/literal-tail-after-4096` | classification: intentumdiff-js-ts-parser#8 |
| `kotlin-ordinary-comment` | classification: kotlin-ordinary-comment-is-expression |
| `odin-body-operator` | classification: intentumdiff-odin-parser#4 |
| `qsharp-body-operator` | classification: intentumdiff-qsharp-parser#8 |
| `qsharp-declaration-api` | classification: intentumdiff-qsharp-parser#8 |
| `qsharp-literal-value` | classification: intentumdiff-qsharp-parser#8 |
| `qsharp-quoted-comment-marker` | classification: intentumdiff-qsharp-parser#8 |
| `qsharp-unicode-payload` | classification: intentumdiff-qsharp-parser#8 |
| `rust-async-modifier` | classification: intentumdiff-rust-parser#4 |
| `rust-reference-mutability` | classification: rust-reference-type-mutability-erased |
| `rust-unsafe-modifier` | classification: intentumdiff-rust-parser#4 |
| `swift-ordinary-comment` | classification: swift-ordinary-comment-is-expression |
| `vbnet-access-modifier` | classification: intentumdiff-vbnet-parser#6 |
| `vbnet-quoted-apostrophe` | classification: intentumdiff-vbnet-parser#6 |
| `wast-import-field` | classification: intentumdiff-wat-parser#4 |
| `wat-import-field` | classification: intentumdiff-wat-parser#4 |
| `web-asciidoc-image_alt` | classification: intentumdiff-asciidoc-parser#6 |
| `web-asciidoc-include_filter` | classification: intentumdiff-asciidoc-parser#6 |
| `web-astro-child_text` | classification: astro-static-template-text |
| `web-astro-external-button-text` | classification: astro-static-template-text |
| `web-astro-preservation` | classification: embedded-javascript-quote-equivalence |
| `web-astro-unicode` | classification: astro-static-template-text |
| `web-css-external-keyword-case` | group reference: final-group-index-integrity |
| `web-css-preservation` | classification: css-comment-misparsed-property |
| `web-generic-incomplete` | source bounds: intentumdiff-generic-parser#4 |
| `web-html-pre_whitespace` | classification: intentumdiff-core#247 |
| `web-latex-class_option` | classification: intentumdiff-latex-parser#4 |
| `web-latex-external-article-option` | classification: intentumdiff-latex-parser#4 |
| `web-latex-package_option` | classification: intentumdiff-latex-parser#4 |
| `web-latex-same_line_environment` | classification: intentumdiff-latex-parser#4 |
| `web-latex-trailing_prose` | classification: intentumdiff-latex-parser#4 |
| `web-markdown-external-code-block-form` | classification: markdown-code-block-equivalence |
| `web-markdown-external-heading-tab` | classification: markdown-atx-heading-semantics |
| `web-markdown-heading_level` | classification: markdown-atx-heading-semantics |
| `web-markdown-preservation` | classification: markdown-atx-heading-semantics |
| `web-mdx-boolean_prop` | classification: intentumdiff-mdx-parser#7 |
| `web-mdx-export_value` | classification: intentumdiff-mdx-parser#7 |
| `web-mdx-external-closing-heading-marker` | classification: markdown-atx-heading-semantics; MDX closing marker |
| `web-mdx-heading_level` | classification: intentumdiff-mdx-parser#7 |
| `web-mdx-import_binding` | classification: intentumdiff-mdx-parser#7 |
| `web-postscript-closing_line_body` | classification: intentumdiff-postscript-parser#6 |
| `web-postscript-inline_procedure` | classification: intentumdiff-postscript-parser#6 |
| `web-postscript-preservation` | classification: postscript-intertoken-whitespace |
| `web-scss-external-escaped-string-spelling` | classification: scss-multiline-style-value-projection |
| `web-scss-external-escaped-string-value` | classification: scss-multiline-style-value-projection |
| `web-svelte-head_script_src` | classification: intentumdiff-svelte-parser#7 |
| `web-svelte-module_context` | classification: intentumdiff-svelte-parser#7 |
| `web-svelte-preservation` | classification: embedded-javascript-quote-equivalence |
| `web-svelte-text` | classification: svelte-static-template-text |
| `web-svelte-unicode` | classification: svelte-static-template-text |
| `web-tsx-disabled_prop` | classification: tsx-boolean-prop-loss |
| `web-vue-external-scoped-color` | classification: intentumdiff-vue-parser#6 |
| `web-vue-external-scoped-css-formatting` | classification: intentumdiff-vue-parser#6 |
| `web-vue-inline_script` | classification: intentumdiff-vue-parser#6 |
| `web-vue-inline_template` | classification: intentumdiff-vue-parser#6 |
| `web-vue-script_src` | classification: intentumdiff-vue-parser#6 |
| `web-vue-style_scope` | classification: intentumdiff-vue-parser#6 |
| `web-vue-unicode` | classification: intentumdiff-vue-parser#6 |

## Final issue-draft review

Independently reviewed all 32 proposed new issue bodies and their source/output contracts: seven compiled, seven data/config, eight dynamic, ten web. Exact structured source blocks and raw artifact hashes agree with retained evidence. The four issue-map files jointly cover exactly 129 cases with no missing, extra, duplicate, empty or unresolved draft references: all 126 final failed/invalid rows plus ABAP, Elixir and TSX's three passing-flag source-fact defects. The 32 drafts cover 48 distinct cases (45 failed/invalid and three manual); the other 81 failed/invalid cases reuse existing issues. Root owns actual filing and durable resolved URLs.

CSS and Python final-index integrity evidence reuses core#254; the TSX absolute-offset emitter has a separately scoped producer draft. No new ticket is justified only by parent containment. Core source-inspection pins are distinct from independently measured native ABI identity. No blocking runner, snapshot-integrity or proposed-issue evidence finding remains after the corrections above. Raw failed/invalid rows affect 55 IDs; including the Elixir explanation defect identifies product evidence defects in 56 IDs. Elixir's passing flag cells do not mean its explanation passed.

## Final frozen-scorecard stamp

The final durable scorecard and compact findings were independently reconciled against the original run, all 760 fixture source/expected records, all 74 per-ID summaries, every confirmed-case output excerpt, resolved issue maps and raw-artifact hashes. Added report fields preserve the original runner records; compact change nodes remove children while retaining truthful source facts. All 129 confirmed cases resolve correctly to 66 distinct case issue URLs: 32 new issues and 34 existing issues. The parent creation log has exactly the 32 reviewed draft keys/URLs. The eight internal fallback investigations remain outside the confirmed-case denominator.

The TSX primary manual finding is correctly the filed core#277 absolute-offset defect; the final report does not substitute another metadata observation for it. Evidence-class totals (488/169/103/0), 732 unique byte pairs, 760 per-ID executions, source-admission gaps and unscored lanes are explicit. No zero-case accuracy rate, full external admission, language certification or all-passing-explanation claim appears. Immutable publication commit `21ce7edf4f27ffd0626da19ca6629df8fbc2af9a` was independently read through the GitHub connector for all four family files; each remote Git blob matches the executed local fixture bytes.

Frozen reviewed files:

- `docs/evidence/2026-11-08-language-assessment.json`: SHA256 `d5ea13c5d6e8224f09b7d2b77da1b5d3f60dc408cdc836ba4687b1be3ff02a7d`.
- `docs/evidence/2026-11-08-language-assessment.md`: SHA256 `4ef6761964c16ee1a0bf4517f8b1a5a1e5e177ac83727246ae5dad74449aeb9d`.
- `docs/evidence/2026-11-08-language-assessment-findings.json`: SHA256 `943ad57d65cf8c4ecc64704f93d9d1b26630c580b961e1c989cc8234a2b71389`.

**Final disposition: ready for durable publication as a bounded diagnostic assessment. No blocking corpus, runner, count, source-identity, issue-map or scorecard finding remains.** Product defects and the stated unassessed dimensions remain open; this stamp approves the evidence report, not product correctness or release readiness.

## What remains unassessed

The broad-source target in core#268 remains open: 35 IDs have no current external-derived input; all IDs have no admitted upstream-gold edit set. Single-source upstream tests are adapted micro-probes, not full suite runs. Repository/task holdouts, mutation coverage, industrial change-family targets and compatibility profiles have not been established for every language. C/C++, JS/TS/TSX, DAX/M, WAT/WAST and Databricks aliases must not disappear into shared-component averages.

No per-language full compiler/interpreter/browser/DB conformance claim is made. Most cases are source/specification-judged; full reference runtimes and enclosing dependencies were not executed for all 74 languages. Exact node-kind/typed-fact coverage, correspondence precision/recall, refactoring correctness, explanation-fact precision/recall and exact span accuracy remain unscored. Source endpoint validity is narrower than source-span correctness; the passing ABAP and TSX examples demonstrate the gap. Empty semantic output on a meaningful edit is a failure, not unsupported evidence to be dropped.

There is no sustained-load, memory-cap, fuel-limit, depth/size-fuzzing, latency-percentile or Windows/runtime-matrix certification. Linux process peak RSS and bounded per-language wall time are measurements. Python DTO/CLI and editor parity, rendering accuracy and downstream group consumers are outside this direct-Rust comparison. They need separate identical-byte validation before product-level completion claims.

Review disposition: accept the corrected corpus and runner as a transparent diagnostic baseline with the final reviewed snapshot reconciled. Do not treat its passing cells as language certification or close the all-language external-admission requirement. Preserve the confirmed failures and passing-row evidence defects in the parent's durable issue ledger.
