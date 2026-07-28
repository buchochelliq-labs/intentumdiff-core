# Agent instructions — intentdiff-core

This is the IntentDiff **engine**: the complete shared backend. Every binding is thin.

## Hard invariants
- New processing logic lands HERE, never in a binding repo. If Go would need it, it's engine.
- The C ABI ([docs/C_ABI.md](docs/C_ABI.md)) is the only language boundary; every handler
  delegates to a plain-Rust `*_impl`. Adding a function = impl + dispatch arm + envelope test.
- Determinism is a contract: trees, hashes, 0-based positions, change ordering.
- No network I/O in the engine; Wasm guests stay sandboxed (fuel/memory limits).

## Build + test (Rust 1.93.0)
```bash
cd crates/rust-core-host && cargo test          # 178 + 25 tier-c (needs INTENTDIFF_TEST_WASM_DIR)
cargo build --release --target wasm32-wasip2    # workspace wasm members, from repo root
cd crates/cli && cargo build --release
```
Details + gotchas (Windows rustc flake, feature gates): [docs/BUILDING.md](docs/BUILDING.md).

## Map
`docs/ARCHITECTURE.md` (pipeline + crate topology) · `docs/C_ABI.md` (binding contract) ·
`.claude/skills/` (engine dev skills — working documentation, keep current) ·
`SECURITY.md` · `SUPPORT.md` (issue routing across the repo family).
