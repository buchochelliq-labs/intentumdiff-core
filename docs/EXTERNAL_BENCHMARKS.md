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
checked against the committed registry, and the C/C++ component comes from the
specific successful CI artifact recorded in the lock. The stager rejects dirty
registry inputs, missing components and digest mismatches. Artifact expiration is
an explicit acquisition failure; there is no fallback to a newer build.

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
