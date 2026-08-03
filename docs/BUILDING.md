# Building intentdiff-core

Toolchain: **Rust 1.95.0** (pinned in CI); target `wasm32-wasip2` for the Wasm components.

```bash
rustup toolchain install 1.95.0
rustup target add wasm32-wasip2
```

## The engine (native cdylib + rlib)

```bash
cd crates/rust-core-host
cargo test                     # Tier-A pins (see the tier-c-wasm note below)
cargo build --release          # the cdylib (intentdiff_rust_core.{dll,so,dylib}) + rlib
```

- **tier-c-wasm tests** (25 of 203) load staged parser components. Without them:
  `cargo test --no-default-features` (they report as ignored). With them: set
  `INTENTDIFF_TEST_WASM_DIR` to a directory of built parser `.wasm` files.
- **Windows release-build flake:** heavy concurrent builds can crash rustc with
  `STATUS_STACK_BUFFER_OVERRUN` / `handle_alloc_error` (a memory blowup in one giant codegen
  unit). Re-run alone, or lower pressure with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`.

## The CLI

```bash
cd crates/cli
cargo build --release          # -> target/release/intentdiff
./target/release/intentdiff engine   # prints the native banner
```

The CLI resolves the bundled parser dir via `--wasm-dir`, then `$INTENTDIFF_WASM_DIR`, then a
directory next to the binary.

## The Wasm components (index engine + renderers)

```bash
# from the repo root (the workspace)
cargo build --release --target wasm32-wasip2
```

Artifacts land in `target/wasm32-wasip2/release/*.wasm`. The SDK dependency is a git dep on
[intentdiff-plugin-sdk](https://github.com/buchochelliq-labs/intentdiff-plugin-sdk) `v0.1.0`;
building against a private clone needs `CARGO_NET_GIT_FETCH_WITH_CLI=true`.
