# External-benchmark scorecard: first slice, 2026-11-17

This is the first scorecard produced under the core #268 contract (schema-2 admission ledger). It covers
the first #261 slice: HumanEvalPack tasks 0–11 in Python, JavaScript, Java, Go, C++ and Rust, plus
handwritten C and C++ character-literal regressions. **It does not certify any language.** 68 of the 74
registered IDs have no executed external case and appear as `GAP` in every lane.

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
