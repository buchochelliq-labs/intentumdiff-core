# Thin public API migration evidence

The Rust executable `crates/rust-core-host/examples/migration_probe.rs` links the
supported Rust API directly. Python uses the installed/source package's public
`review_text` and `SemanticDiffer.diff_strings` APIs. Utility cases additionally
compare native typed helpers with the binding ABI. The example does not call the
C ABI or Python. Actual output is in `actual-diffs.json`.

The corpus expectations were written from source examples, not generated outputs:
- Rename a heading: retain old/new heading evidence.
- Rename and edit body: retain both edits.
- Duplicate headings: do not hide the independent body edit (pairing can be noisy).
- Insert a section: no spurious movement.
- Fenced text: not a document heading.
- Move a section: MOVE without claiming the same bytes are ignored trivia.
- UTF-8 text/section spans: byte columns (été is five bytes, three characters).
- Schema-keyed reorder: only pay_v1 → pay_v2 changes, at exact source positions;
  no unrelated additions/deletions/moves.
- Explicit modified status wins over an empty old/new file.
- Filename classes preserve fnmatch semantics; regex set operators are literals.

Run from the Python repository with the current core library and parser components:

```sh
cargo build --manifest-path ../intentumdiff-core/crates/rust-core-host/Cargo.toml --example migration_probe
python scripts/check_migration_parity.py \
  --core-dir ../intentumdiff-core \
  --native ../intentumdiff-core/crates/rust-core-host/target/debug/examples/migration_probe \
  --wasm-dir src/intentumdiff/wasm \
  --evidence ../intentumdiff-core/docs/evidence/thin-api/actual-diffs.json
```

Only execution-route metadata, tree IDs/hashes and audit-only NOISE_SUPPRESSED
counts are excluded from semantic comparison. Full outputs remain available for
independent review. Different stages can assign different internal IDs without
changing node positions or evidence. The JSON includes temporary paths/timings from
the actual run; these are diagnostic, not stable expectations.

Independent review reproduced and required fixes for Unicode byte columns and
fnmatch character-class compatibility, in addition to the initial lifecycle and
Markdown evidence defects. Each has red→green regression coverage. No GUI/VSIX
acceptance, merge, tag, release or cross-platform local build is claimed.
