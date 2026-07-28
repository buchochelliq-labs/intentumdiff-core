# Third-party notices

This repository vendors patched copies of third-party crates under `crates/patches/`
(build-script stabilization; see each crate's own license file within its directory):

- `quote`, `serde_core`, `serde_json`, `zmij` (MIT OR Apache-2.0)
- `wit-bindgen-rust-macro` (Apache-2.0 WITH LLVM-exception)
- `tree-sitter` (MIT)

All other dependencies are consumed unmodified from crates.io under their declared licenses
(see `Cargo.toml`/`Cargo.lock` per crate). A formal license scan precedes any commercial
distribution (see [docs/OPEN_CORE.md](docs/OPEN_CORE.md)).
