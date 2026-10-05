# FullParse host-utils probe

This component is a test fixture, not a shipped parser. It declares FullParse and
imports the engine's existing WIT contract directly. Its `process` export returns
the imported utility result so the host test can inspect it before SemanticTree
decoding. It contains no trivia traversal or hashing implementation.

Build and run from the repository root:

```sh
rustup target add wasm32-wasip2
cargo build --locked --manifest-path crates/rust-core-host/tests/fixtures/host-utils-guest/Cargo.toml --target wasm32-wasip2
cargo test --manifest-path crates/rust-core-host/Cargo.toml --no-default-features --features host-utils-guest host_utils_guest
```

CI builds this guest and enables the gate for both maintainer and fork runs. A
missing guest fails the gate. The fixture is excluded from shipping workspaces.

The permanent `../host_utils.json` examples specify expected results separately
from either host call:

- A root whose type is trivia becomes JSON null; returning the original root
  would retain content explicitly requested for removal.
- Stripping a comment child preserves its non-trivia sibling and tree shape.
- `type`/`text` and `node_type`/`label` represent the same supported leaf. Both
  hashes must equal SHA-256 of the UTF-8 bytes of `identifier:café`, following the
  documented leaf formula. Hash equality by itself is insufficient.
- Malformed JSON, scalar nodes, and invalid child collections fail explicitly.

Generated boundary cases cover byte, nesting, node-count and trivia limits. An
adversarial operation ignores the utility's error and returns `{}`; the production
host must still reject it for both the old and new source calls.

This gate covers the native Rust Wasm host and direct Rust operations. It does
not by itself certify the Python-hosted guest callback path or whole-RC acceptance.
