# Native selection validation

Scope: core #108 native host adoption and #122 empty Python module labels.

- `cargo test --manifest-path crates/rust-core-host/Cargo.toml --no-default-features --tests`: 333 passed; 25 existing component tests ignored by the feature flag.
- With the registry-pinned Python component and `INTENTUMDIFF_TEST_WASM_DIR`, `cargo test --manifest-path crates/rust-core-host/Cargo.toml --lib parser_registry`: 6 passed, including real metadata/probes, allowlist, custom Python identity, fuel errors through single/commit/parse entry points, and source corpus.
- Python native-selection, shared-content, shared-filename, lazy-registry, Rust-adapter and detection suites with `INTENTUMDIFF_NATIVE_PROBE`: 114 passed. Includes real Python and Dockerfile components.
- Source judgment: return 1 → return 2 has one integer modification and one meaningful group; create/delete `value = 1` has only additions/deletions; Dockerfile changes only its base image version and leaves RUN unchanged.

Local Rust linking used test/dev `CODEGEN_UNITS=1`, debug=0 and incremental=0;
these are environment-only settings. Hosted platform and wheel CI remains a gate.
