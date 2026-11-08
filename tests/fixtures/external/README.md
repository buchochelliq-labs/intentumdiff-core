# External correctness fixtures

Schema version 1 follows the approved correctness-foundation plan. This is a small, explicitly qualified pilot, not certification of all languages. `ledger.json` lists every routed ID from `inventory.json`; primary corpus selections reproduce core issue #268. Six languages have pilot byte-identity evidence; the other 68 have explicit acquisition/admission gaps. C has supplementary handwritten literal regressions only.

HumanEvalPack is pinned to `9a41762f73a8cb23bb5811b73d5aab164efcf378`. The six original Parquet files are retained unmodified, together with the pinned dataset card and all original fields for the first twelve physical rows (task IDs 0–11). Original upstream strings are UTF-8 encoded without rewriting; each program concatenates `declaration` and the relevant solution. Go additionally prepends `package main\n\n` and the original `import` field. Original tests and environment fields remain in each task JSON; they are not silently discarded or substituted for the parser input. These are specialist research repair tasks with translated task-level overlap across languages, so portfolio totals must deduplicate task IDs 0–11.

There are 167 cases: 72 repair pairs, 72 byte-identity controls, six formatting controls, six public-function rename controls (one each in the six pilot languages), seven C/C++ literal regressions, and four matching C/C++ identity/formatting controls. Of these, 71 repairs remain seed-only (`expected: null`, `required: false`). Source bytes alone do not establish an engine change-fact oracle. CPP/11 is independently admitted: its complete original upstream test compiles with g++ C++17; the buggy program fails its first assertion and canonical program passes. Its two exchanged literal payloads were inspected directly in the original sources. Required facts carry independently computed original byte positions and permit evidence within a reported changed ancestor; they do not require a particular diff-node granularity. Identity is proven by identical paths and hashes. The public API does not specify `is_style_only` for an empty edit, so exact byte-identity controls use `is_style_only: null` while still requiring no semantic changes and forbidding reported change facts. All six formatting/rename transformations have independently checked exact source deltas; Python additionally uses AST comparison, JavaScript passed Node syntax checks, and C++ passed g++ C++17 syntax checks. Java/Go/Rust control compiler execution is not claimed. the rename changes the public binding and is not style-only. Handwritten cases remain separate from the external-gold denominator.

The original retained API viewer sample was not independently recovered. No claim that the downloaded Parquet bytes equal an unavailable pilot viewer sample is made.

Provenance, exact file SHA-256 and attribution are in `manifest.json`. The pinned dataset card declares MIT and contains the OctoPack citation and authors; its text is retained in `upstream/README.md`. A separate upstream LICENSE text was not supplied with this acquisition, so this records the dataset's explicit license declaration rather than claiming a separate license audit. The routing inventory comes from intentumdiff-python commit `14119103c46f08cedc8b4313b1832c8daa511553`; it describes route IDs/components, not an installed parser-artifact identity. Runtime reports must record the actual staged component hashes separately.

Validate offline with the standard library:

```sh
python scripts/import_external_benchmarks.py --validate
python -m unittest discover -s tests/tooling -p test_external_benchmark_import.py
```

Replay the full import with PyArrow installed (the optional replay test also exercises this):

```sh
python scripts/import_external_benchmarks.py --data-dir tests/fixtures/external/upstream --inventory tests/fixtures/external/inventory.json
```

The importer refuses changed Parquet SHA-256 before extracting rows. It does not download data, run IntentumDiff, or synthesize expectations from engine output. No product semantics live in this Python test tooling.
