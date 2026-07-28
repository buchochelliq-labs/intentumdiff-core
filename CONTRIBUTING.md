# Contributing to intentdiff-core

## Ground rules

- **The engine is the single implementation.** New processing logic lands here — never in a
  binding. If a Go binding would need it, it belongs in this repo.
- **The C ABI is the only language boundary** ([docs/C_ABI.md](docs/C_ABI.md)): every handler
  delegates to a plain-Rust `*_impl`, so in-process consumers and FFI bindings never diverge.
  Adding a function = write the `*_impl`, wire a dispatch arm in `c_abi.rs`, extend the
  envelope tests.
- **Determinism is a contract.** Trees, hashes, positions (0-based), and change ordering must
  be reproducible; tests pin behavior, not implementation.

## Workflow

1. Build + test per [docs/BUILDING.md](docs/BUILDING.md) — `cargo test` in
   `crates/rust-core-host` must be green (203/203 with staged components, 178 + 25 ignored
   without).
2. Wasm members and the CLI build clean (`cargo build --release --target wasm32-wasip2`;
   `cargo build --release` in `crates/cli`).
3. Zero new warnings; no `unsafe` beyond the audited FFI boundary.
4. The `.claude/skills/` directory carries the engine development skills (dev-loop, engine,
   testing…) — they are working documentation; keep them current when behavior changes.

## Security

See [SECURITY.md](SECURITY.md). Wasm guests run sandboxed (fuel + memory limits, no ambient
capabilities); registry refs and dep hashes are validated by the shared #88 controls in
`registry.rs`; subprocess calls (git/hg/svn/p4) go through argument-injection guards.
