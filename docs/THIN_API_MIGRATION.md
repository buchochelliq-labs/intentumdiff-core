# Migration ownership and scope

| Concern | Rust owner | Python responsibility |
|---|---|---|
| Schema/provider discovery, profile matching, validation, identities | schema_profiles | Fetch/cache/read supplied schema and descriptor bytes; DTO adaptation |
| Native local profiles | schema_context + schema_profiles | Existing Python host paths remain supported |
| Wasm strip-trivia and structural hashing | host_utils | Callback marshaling |
| Lifecycle classification/finalization | lifecycle | Pass source/status; construct SemanticDiff |
| Markdown reconciliation/evidence | markdown_review | Pass presentation DTOs |
| Compile database selection, argv/flags, fingerprint | compile_context | Read file; native host has equivalent discovery |
| Complete parser-free review | api::review_text | review_text returns SemanticDiff |
| Complete parser-backed review | api::review_sources / shared finalization | SemanticDiffer host integration |

Existing Python public APIs stay compatible. Rust review results, nodes, groups,
positions, options and errors are typed, with extensible metadata. The C ABI remains
the sole foreign-language boundary; Go/Java reuse it. Maturin/cffi packaging is
unchanged. VS Code uses external Python/CLI today.

This implements the bounded owners #97, #98, #99, #55 and #100; #101 gates output
validity and linked Python#57. It does not claim that all broader migration work is
finished. Catalogue authority (#39), shared trust policy (#40), config/guardrail
loading (#41), full native CLI parity (#42) and existing semantic edge cases remain
tracked separately. Legacy Python comparison helpers are not a production fallback;
removing all dead compatibility helpers is distinct from moving active processing.

Compile metadata does not execute a compiler or change parsing. Native schema
resolution is local by default; hosts can supply fetched schema context. Descriptor
keyed-array hints retain the existing identity-field model, not a new schema validator.
See C_ABI.md and evidence/thin-api/README.md for contracts and reproducible checks.

## Native parser selection (#108)

The bundled native resolver now drives `filename_selection::next_action`, the
same public Rust policy that Python drives over the C ABI. Manifest records are
discovery hints; aliases sharing a component are loaded once, then guest grammar,
languages, priority and detection results determine selection. Missing components
can be skipped; runtime traps and probe failures escape as errors. Invalid
manifests fail explicitly. Single-file review, commit review and standalone parse
all use this host adapter. Certified Python execution remains available only after
real component selection and the existing first-party identity check.

Source fixtures cover a Python literal edit, creation/deletion and Dockerfile
image changes. Python/native tests exercise a misleading manifest candidate that
actually declines, a missing component, terminal fuel failures, and the resulting
diffs. Empty Python modules retain their structural label (#122), preventing a
spurious module-name modification when source is created or emptied.

This does not certify third-party interpret-CST execution or shared trust policy;
those host/execution concerns remain separate from deterministic selection.
