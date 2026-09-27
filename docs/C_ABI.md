# The IntentumDiff C ABI — the binding-author contract

The engine ships as a native shared library (`intentumdiff_rust_core.{dll,so,dylib}`) exposing
**one stable language boundary**. Every binding — Python (ctypes), Go, Java (FFM), and any
future language — drives the identical surface. Bindings do zero functional work.

## The two exports

Semantic source positions use zero-based lines and UTF-8 byte columns, matching parser spans.
Bindings must not reinterpret columns as character indices.

Additional semantic helpers:

| Handler | Positional arguments | Result |
|---|---|---|
| `resolve_references` | definition-array JSON string, usage-array JSON string | usages with unique exact-name definition attached; ambiguity clears resolution |
| `empty_semantic_tree` | language string | canonical empty source tree (native: `lifecycle::empty_tree`) |
| `complete_routed_review` | request JSON string: finalized tree changes, source/trees, filenames, language, optional schema/compile metadata | complete review DTO |
| `parse_guardrail_policy` | YAML/JSON source string | normalized protected-rule array |
| `apply_guardrail_policy` | request JSON string with diff, rules, old/new source and trees | complete diff with violations and guardrail metadata |
| `enrich_literal_labels` | tree JSON string, source string | enriched tree |
| `review_trees_equivalent` | old tree JSON string, new tree JSON string | boolean |

These share the engine implementations used by native routes. Malformed trees return the
normal error envelope. Equivalence preserves whitespace in string and character values.

```c
char *intentumdiff_call(const char *name, const char *args_json);
void  intentumdiff_free(char *ptr);
```

- `name` — the engine function to invoke (UTF-8 C string).
- `args_json` — a **JSON array of positional arguments** (`"[]"` for none — never `null`).
- Returns a heap-allocated UTF-8 JSON **envelope** which the caller MUST release with
  `intentumdiff_free`. A `NULL` return means an allocation/encoding failure only.
- The boundary catches panics (unwinding across `extern "C"` is UB) and reports them as
  `internal` errors.

## The envelope

```json
{"ok": true,  "result": <value>}
{"ok": false, "error": "<message>", "error_type": "<slug>"}
```

`result` is a parsed JSON value (string results are JSON strings; native results — booleans,
lists — are their JSON forms).

### `error_type` slugs (verified by the binding test suites)

| Slug | Meaning | Typical binding mapping |
|---|---|---|
| `not_found` | an absent path / blob / working-tree file | `FileNotFoundError` / `fs.ErrNotExist` |
| `value_error` | invalid input — including a **missing positional argument** (`"missing argument 0 (language)"`) | `ValueError` |
| `bad_request` | the **args_json itself is malformed** (not a JSON array, e.g. `null`) | `ValueError` |
| `internal` | an engine panic caught at the boundary | `RuntimeError` |

Two pinned behaviors binding authors rely on (see the Go/Java scaffold tests):
- **Extra arguments on a zero-arg call are ignored** — the positional readers only consume
  what they need.
- A nil/None argument list must be marshalled as `[]`, not `null` (`null` → `bad_request`).

## Argument conventions

- Arguments are positional; JSON-string-valued parameters (trees, configs, requests) are passed
  as JSON strings *inside* the args array.
- Byte parameters (e.g. content sniffing) are passed as JSON arrays of integers.
- The two commit functions (`diff_batch_commit_json`, `diff_working_tree_python_commit_json`)
  return a **commit-tuple envelope**: `{"control": <json>, "commit_diff_json": <string|null>}` —
  the certified CommitDiff serialized as UTF-8 JSON, marshalled without re-parsing.

## Function surface

The dispatch table lives in `crates/rust-core-host/src/c_abi.rs` — every handler delegates to
the crate's plain-Rust `*_impl` functions, so the ABI and any in-process Rust consumer (the CLI,
the live-server) always run the same code. Handler names match the historical Python binding
names with the `_json` suffix dropped (the two commit functions keep it).

## Reference bindings

- Python: [`intentumdiff-python`](https://github.com/buchochelliq-labs/intentumdiff-python) (`src/intentumdiff/rust_core.py` — `_CtypesBackend`)
- Go: [`intentumdiff-go`](https://github.com/buchochelliq-labs/intentumdiff-go)
- Java: [`intentumdiff-java`](https://github.com/buchochelliq-labs/intentumdiff-java)

### Incomplete-source review (core #21)

`parse_errors_present(source, tree_json, language)` returns a boolean. It examines
raw CST or semantic-tree error/missing markers; Python also uses the native parser
because semantic plugins can prune those markers. Call before trivia equivalence.
Malformed tree JSON is an error. Source is bounded to 4 MiB and tree JSON to 16 MiB.

`source_fallback_diff(old_source, new_source, old_filename, new_filename, language,
reason)` returns a complete diff with `is_fallback=true`. It compares exact UTF-8
source, preserving indentation, line endings and literal whitespace. It emits one
contiguous range spanning all edits (possibly including unchanged text between
edits), found by equal character prefix/suffix. `metadata.source_ranges` contains
exclusive UTF-8 byte offsets on each side; node positions use zero-based lines and
byte columns. Node labels preview at most 160 characters, while ranges and hashes
cover the entire changed region. Consumers retain original sources for full text.

Changed input is review-worthy with confidence 0.5 and semantic equivalence unknown;
it is never called refactoring or style-only. Identical input has no changes. The
operation is linear and accepts at most 4 MiB per source. `reason=parse_errors`
adds a parse diagnostic; other decline reasons do not invent parse failures.

Native Python batches return `COMPLETE` with this fallback diff, rather than a
control-plane `FALLBACK` request for a binding to perform comparison. The
`fallback_to_token_diff` configuration name remains compatible but now selects
Rust source comparison. `finalize_review` can likewise return `fallback_diff`;
bindings must preserve that full payload and only attach filenames/lifecycle.

Build provisioning searches up to ten pages of 100 successful workflow runs at
the registry-pinned commit. GitHub-generated `dynamic/` jobs are ignored; they do
not publish project parser artifacts. Commit and component-checksum checks remain
mandatory. This prevents scheduled-job history from hiding a still-valid build
(core #54); missing or expired artifacts still fail provisioning explicitly.

## Shared-operation migration

All handlers below retain the same `intentumdiff_call`/`intentumdiff_free` ownership
and error envelope. Python wheels remain maturin/cffi; Go/Java need no Python runtime.

| Handler | Positional arguments | Result |
|---|---|---|
| `review_text` | old source, new source, old filename, new filename | Complete plain-text/Markdown diff |
| `reconcile_markdown` | presentation object, old source, new source, old filename, new filename, phase (`moves`, `renames`, `all`) | Presentation with changes, remapped groups, ignored-style evidence |
| `infer_file_lifecycle` | old source, new source, optional status | `added`, `deleted`, or `modified`; explicit status takes precedence over empty contents |
| `finalize_file_lifecycle` | diff object, lifecycle string | Diff with lifecycle metadata and final flags |
| `host_strip_trivia` | tree JSON string, trivia string array, optional limits object | Tree object, or JSON null if root is trivia |
| `host_structural_hash` | tree JSON string, optional limits object | Deterministic SHA-256 hex string |
| `schema_profiles` | JSON-string request with operation | Operation-specific profile/schema result |
| `compile_context` | request object: database array, database_path, filename, language, cwd | Context object or null for missing/ambiguous exact matches |

`schema_profiles` operations and their typed equivalents are defined in the public
`schema_profiles` module: `discover`, `provider`, `derive`, `parse_documents`,
`validate`, `match`, and `resolve`. Descriptor errors are returned as diagnostics;
malformed operation requests use the normal error envelope. Hosts fetch schema bytes;
engine processing never fetches schemas over the network.

Tree utility defaults cap JSON at 8 MiB, depth at 256 and visited JSON values at
1,000,000; trivia limits are 1,024 names, 256 bytes/name and 64 KiB total. Callers may
tighten these limits, never loosen the shared ceiling. Malformed trees fail explicitly.
Compile context is metadata-only: no compiler is executed and flags do not alter
semantic parsing. Exact paths use target-platform syntax; unrelated basenames never
supply context. Two exact matches are ambiguous and return null.

The supported native facade is `api::review_text` / `api::review_sources`, returning
`Result<api::Review, api::ReviewError>`. Nodes, source positions and groups are typed;
extensible attributes/metadata preserve unknown engine fields. Classification names
are open strings for forward compatibility. `ReviewOptions::settings` accepts the
same configuration keys as native live review. Lower-level Rust operations remain
public for callers supplying their own host I/O. See `examples/migration_probe.rs`.

## Shared guardrail policy

Native Rust consumers use `guardrail_policy::parse_policy` and `guardrail_policy::apply_policy`.
Hosts discover/read policy files; Rust interprets root `protected` and nested `guardrails.protected`
forms, validates rules, evaluates changes, and marks edits to `intentumdiff.yaml` immutable.
Absent severity defaults to `important`; explicit non-string or unsupported severity is an error.
Applicable rules require both semantic trees. Errors must propagate through bindings.

Native callers can use `routed_review::complete` for the same reconciliation as the C ABI.
It owns invariance suppression, generic/Markdown replacement, group indices, style decisions
and evidence. Suppression of non-equivalent sources does not manufacture equivalence evidence.
Partially retained meaningful groups describe only surviving changes; partially invalidated
relationship classifications are discarded. Explicit non-final index spaces are preserved.

Native Rust uses `symbol_index::{SymbolDefinition, ReferenceUsage, ReferencePosition, resolve_references}`.
Resolution matches exact qualified names, never chooses arbitrarily between multiple definitions,
and returns new DTOs without mutating the input usages. Python retains ordinary map lookup and
DTO transport; it does not decide uniqueness. Malformed ABI arrays return errors.
