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
| `reconstruct_patch` | request JSON string: patch_text, optional original_content/filename, require_complete | old/new content, filename, scope, optional warning |
| `resolve_references` | definition-array JSON string, usage-array JSON string | usages with unique exact-name definition attached; ambiguity clears resolution |
| `empty_semantic_tree` | language string | canonical empty source tree (native: `lifecycle::empty_tree`) |
| `complete_routed_review` | request JSON string: finalized tree changes, source/trees, filenames, language, optional schema/compile metadata | complete review DTO |
| `parse_guardrail_policy` | YAML/JSON source string | normalized protected-rule array |
| `apply_guardrail_policy` | request JSON string with diff, rules, old/new source and trees | complete diff with violations and guardrail metadata |
| `live_negotiate_protocol` | hello request JSON string | hello success (`protocol_version`, `supported_protocol_versions`) or a protocol error: absent/null version = current (2); a supported integer is accepted; another integer → `unsupported_protocol_version`; a non-integer → `invalid_protocol_version` |
| `guardrail_check_result` | request JSON string: `diffs` XOR `violations`, required `strict`, optional `checked_files`, `metadata` | complete guardrail check verdict (see below) |
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
| `size_limit` | an input, CST/tree or parser output exceeded a configured byte limit; the message names the diff-config setting that raises it (`max_cst_bytes` or `max_plugin_output_bytes`) and contains `size limit exceeded` | `ValueError` subclass / a "too large" file error |
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

### Commit cross-file index completeness (core #298)

One changed file that cannot be indexed (parser fuel exhausted, parser error, malformed tree)
no longer discards the commit's cross-file analysis. The file is excluded from **both** symbol
tables (so none of its symbols are reported as moved/deleted), and the result says so
explicitly. Every commit-level producer publishes the same four fields:

| Field | Type | Meaning |
|---|---|---|
| `cross_file_index_status` | `"complete"` \| `"partial"` | `"partial"` whenever `index_errors` is non-empty |
| `cross_file_index_complete` | boolean | `true` only when every changed file was indexed |
| `indexed_file_count` | integer | changed files that contributed to the index |
| `index_errors` | array | one object per unindexed file (below) |

Each `index_errors` entry: `{"path", "old_filename", "new_filename", "language", "side"
("old" \| "new" \| "both"), "code", "reason"}`. `code` is stable and machine-readable —
`fuel_exceeded`, `output_limit_exceeded`, `malformed_tree` or `parser_error` — so callers never
parse `reason` text. `cross_file_changes` proves "no cross-file change" only when the status is
`"complete"`; with `"partial"` it covers the indexed files only. A missing/unloadable parser
component or a table/diff fault is not per-file and still fails the call.

- `live_handle_review` puts the four fields in the response `metadata` (next to
  `review_fuel_capped`), beside the unchanged `commit_diff`.
- `commit_cross_file_index(files_json)` serves hosts that parse each file themselves (Python
  `CommitDiffer.finalize_commit_diff`). `files_json` is an array of `{old_filename,
  new_filename, language, old_tree, new_tree, old_error, new_error}`: a tree is a SemanticNode
  object or `null` (empty side), an error is the host's parse-failure text or `null`. A file with
  a side error is excluded and reported. Result: the four fields plus `cross_file_changes`.

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
`reason=plugin_parse_error` (core #314) is for a side the parser component could not
read at all, because it returned its in-band `{"error": ...}` envelope instead of a
tree. It also adds a parse diagnostic.

`plugin_parse_error_review(old_source, new_source, old_filename, new_filename, language,
side, message, config_json)` is the review of such a pair. `side` is `old` or `new`, and
`message` is the parser's error. Core decides, and bindings only relay the result:
- with `fallback_to_token_diff` (the default), it returns `{"diff": <SemanticDiff>}`: the
  source fallback with `metadata.fallback_reason = "plugin_parse_error"`,
  `metadata.plugin_error` and `metadata.plugin_error_side`, so a file mid-edit keeps its edit;
- with the fallback disabled, it returns
  `{"declined": true, "fallback_code": "plugin_parse_error", "reason": "plugin returned error (<side>): <message>"}`,
  which a binding raises as the parser error.

When the live/native review (`live_diff_contents`, `live_handle_diff`, the changed-sources
dispatcher) declines because a parser returned that envelope, its `{"fallback": <reason>}`
response carries `fallback_code: "plugin_parse_error"`. The reason keeps the parser's own
message: `<path>: plugin returned error (old|new): <message>`, for example `Empty semantic
tree` for half-typed YAML or XML. Other declines carry no `fallback_code` unless documented
here, such as `native_parse_incomplete`.

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

## Binding policy operations (0.0.3)

Small, pure policy ops a binding needs so it never re-derives an engine rule locally.

### Parser fuel budget (`fuel_budget`)

`fuel_budget(config_json, content_len)` → `{"fuel": <u64>}` or `{"fuel": null}`; native
`api::fuel_budget(config_json, content_len) -> Result<Option<u64>, ReviewError>`.
`config_json` is a diff-config JSON object string (empty string = `{}`) validated exactly as
every diff call validates it; `content_len` is a non-negative integer byte count (the source
being parsed, or the payload handed to a renderer). The policy is the one the engine applies to
every parse:

- `plugin_fuel < 0` (either spelling) is **unlimited** → `{"fuel": null}`. Bindings pass this
  through as "no fuel metering"; they must not substitute a positive budget.
- `plugin_fuel` below 1,000,000 is a deliberate tight budget and is returned unchanged.
- Otherwise `plugin_fuel` (default 10,000,000) is a **minimum**:
  `fuel = max(plugin_fuel, 20_000_000 + 20_000 * content_len)` (saturating; a bounded config
  never becomes unlimited).

Malformed configs, a negative or non-integer `content_len`, or missing arguments return a
`value_error` envelope.

### Registry manifest validation (`validate_registry_manifest`)

`validate_registry_manifest(manifest_json)` → `{"ok": bool, "errors": [{"path": str, "message": str}]}`;
native `api::validate_registry_manifest(&Value) -> Vec<(path, message)>`. The host parses
`registry.yaml` (or JSON) and passes the document as a JSON string. `path` is an RFC 6901 JSON
Pointer into the manifest (`""` = root, `/plugins/<name>/ref`, `/plugins/<name>/capabilities/1`).
Every problem is reported in one pass; wrong JSON types are errors, never exceptions. Only an
unparseable `manifest_json` string is a `value_error` envelope. Rules (patterns are full-string):

- Top level: object with `version` (integer ≥ 1), `plugins` (object keyed by package-safe name
  `^(?:intentumdiff-)?[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`), optional `renderer_components`; no
  other keys. `renderer_components` needs `repository: "intentumdiff-core"`,
  `artifact: "renderer-components"`, a 40-hex lowercase `ref` and exactly the four renderer pins.
- Entry fields: `source` ∈ {`git`, `pypi`} (default `git`), `trust_tier` ∈ {`official`,
  `community`} (default `official`); `ref`/`repo`/`description`/`abi_target`/
  `provenance_manifest_ref` strings; `wasm_checksums` `{<name>.wasm: <64 lowercase hex>}`;
  `dep_hashes` `{package==version: "sha256:<64 lowercase hex>"}`; `allowed_dependencies` string
  array; unknown fields rejected. `abi_target`, when present, is `MAJOR.MINOR.PATCH`
  (no leading zeros). Compatibility with the host contract is still decided at load time.
- `capabilities`: a duplicate-free array drawn from the closed vocabulary derived from the
  pinned WIT — the interface each `world` exports: `parser`, `renderer`, `enricher`,
  `index-engine`, `diff-analyzer` (`host-utils` is an import, not a capability).
- **Official tier** (the default): `git` entries need an immutable 40-hex lowercase commit
  `ref`; `pypi` entries need an exact version `ref` (`^[0-9][A-Za-z0-9.+!-]{0,63}$`);
  non-empty `wasm_checksums`, non-empty `capabilities` and an `abi_target` are required; a
  custom `repo` is rejected (a custom source is community tier by definition).
- **Community tier**: every field is optional (listed, installs unverified); any field that is
  present must still be well-formed — a git `ref` must pass the non-strict
  `validate_registry_ref` traversal guard, a pypi `ref` is empty (latest) or a version.

### Parser allowlist identifiers (`parser_allowlist_matches`)

One rule decides whether an `allowed_plugins` list admits a parser, used by
`filename_selection_next`, `content_detection_plan/finish`, the native bundled resolver
(`allowed_plugins` diff-config key) and bindings' discovery/inventory listings:

- `null`/absent: **every** parser is allowed.
- `[]`: **no** parser is allowed (distinct from `null`).
- otherwise a parser is allowed when an entry exactly equals (case-sensitive; empty strings never
  match) its **grammar id** (loaded `grammar-id`), its **plugin id** (catalogue/opaque plugin
  identity, e.g. `acme/python`), one of its catalogue **aliases**, or one of its declared
  **language ids**. Unknown identifiers select nothing; they are not errors.

`parser_allowlist_matches(allowed_plugins, identity_json)` → `{"matches": bool, "matched_by":
"unrestricted"|"grammar_id"|"plugin_id"|"alias"|"language"|null}` evaluates it for one parser.
`allowed_plugins` is a JSON array of strings or `null` (not a JSON string); `identity_json` is a
JSON string `{"plugin_id", "grammar_id", "languages": [..], "aliases": [..]}` (all optional).
Native: `filename_selection::parser_allowlist_match(Option<&[String]>, &ParserIdentity)`.
Grammar-id allowlists behave exactly as before; plugin, alias and language ids were previously
ignored (selected nothing) and now select their parser.

## Shared guardrail policy

Native Rust consumers use `guardrail_policy::parse_policy` and `guardrail_policy::apply_policy`.
Hosts discover/read policy files; Rust interprets root `protected` and nested `guardrails.protected`
forms, validates rules, evaluates changes, and marks edits to `intentumdiff.yaml` immutable.
Absent severity defaults to `important`; explicit non-string or unsupported severity is an error.
Applicable rules require both semantic trees. Errors must propagate through bindings.

### Content class

Every served review (`SemanticDiff`) carries `content_class`: `code` | `docs` | `config` |
`data` | `text`, from the routed language (`content_class::content_class`; Markdown files on the
generic text route are `docs`). It describes what was reviewed, never a risk. `ChangeGroup`
carries no `impact`: group `kind` and `refactoring_kind` already state everything the engine
has decided about a group, and an impact level would be a risk claim the engine cannot derive.

### Review completeness (core #252)

Every served review (`SemanticDiff`) carries `completeness` and `incomplete`. They are set
at one shared boundary (`completeness::attach`, reached from `routed_review::complete` and
from the lifecycle stamp every served diff passes), so `live_diff_contents`,
`live_handle_diff`, `live_handle_review`, `diff_batch`, `diff_batch_commit_json`,
`complete_routed_review`, `review_text`, `source_fallback_diff` and `finalize_file_lifecycle`
all report the same state. `finalize_review` also returns `completeness` (and
`equivalence_proof`) for bindings that complete the review themselves.

| `completeness` | Meaning | `incomplete` |
|---|---|---|
| `complete` | `changes` are the review: at least one change, or byte-identical sources | `null` |
| `equivalent` | Sources differ and equivalence was proved: `is_style_only=true`, or `is_style_only=false` with `metadata.equivalence_proof` naming the rule (e.g. `keyed_data.unordered_mapping_member_order`, `keyed_data.number_exact_decimal_respelling`) | `null` |
| `incomplete` | Sources differ, no change survived and nothing proved equivalence | object (below) |

Byte-identical input is `complete`, never `equivalent`, so it stays distinguishable from a
proved style-only edit even on routes that also set `is_style_only` for identical input.
An `incomplete` review keeps `changes=[]`, `has_semantic_changes=false`,
`is_style_only=false` and `metadata.no_surviving_changes=true`: the core never fabricates
semantic changes. It is not a clean review, and bindings must not render it as "no changes".

```json
"incomplete": {
  "reason": "unexplained_source_delta",
  "message": "IntentumDiff could not explain this change. ... Please report a bug at https://github.com/buchochelliq-labs/intentumdiff-core/issues/new (reason: unexplained_source_delta; language: go; core: 0.1.0).",
  "report_url": "https://github.com/buchochelliq-labs/intentumdiff-core/issues/new",
  "language": "go",
  "core_version": "0.1.0",
  "source_ranges": [{"old_start_byte": 26, "old_end_byte": 36, "new_start_byte": 26,
    "new_end_byte": 36, "old_start_line": 2, "old_end_line": 4, "new_start_line": 2,
    "new_end_line": 4}],
  "source_ranges_truncated": false,
  "source_ranges_available": true
}
```

The `report_url` above is shown without its query for brevity: it is the pre-filled
`https://github.com/buchochelliq-labs/intentumdiff-core/issues/new?title=…&body=…` URL of
`report.issue_url` (described below).

- `reason` is a stable code; `message` is core-authored text that every binding (Python,
  VS Code, LSP, live server, CLI) shows verbatim. It names only the routed language id and
  the core version: no filename, label, identifier or source text.
- `source_ranges` are the changed line hunks (zero-based lines, exclusive end bytes),
  capped at 64 with `source_ranges_truncated`. `source_ranges_available=false` only when a
  source-less stage (`finalize_file_lifecycle` on a DTO without a prior verdict) had to
  classify from `metadata.no_surviving_changes` alone.
- `api::Review` exposes `completeness: Option<String>` and `incomplete: Option<Incomplete>`
  (`Incomplete.attributes` carries `report`).

#### Sanitised repro outline (`incomplete.report`)

Core builds one bug-report outline so every binding scrubs identically:

```json
"report": {
  "title": "Incomplete review: unexplained_source_delta (go, core 0.1.0)",
  "body_markdown": "### IntentumDiff could not explain a change\n…",
  "truncated": false,
  "issue_url": "https://github.com/buchochelliq-labs/intentumdiff-core/issues/new?title=…&body=…",
  "outline": {
    "schema_version": 1, "reason": "unexplained_source_delta", "core_version": "0.1.0",
    "language": "go", "plugin": {"id": "go", "version": "sha256:4b60…"},
    "regions": [{"index": 1,
      "old": {"start_line": 4, "start_col": 0, "end_line": 4, "end_col": 0, "bytes": 0},
      "new": {"start_line": 4, "start_col": 0, "end_line": 5, "end_col": 0, "bytes": 13},
      "old_skeleton": "source_file > function_declaration > block",
      "old_shape": "block(statement_list(expression_statement(call_expression[id1](id1, argument_list(string(9), int))), …))",
      "new_skeleton": "…", "new_shape": "…"}],
    "regions_truncated": false, "structure_available": true,
    "suppressions": [{"pass": "refine:suppress_descendant_noise",
      "removed": [{"change_type": "MOVE", "node_type": "call_expression", "count": 2}], "added": []}],
    "placeholder_count": 2
  }
}
```

- **Contents.** Core version, routed language id, parser plugin id and version (the
  component's SHA-256, since the manifest records no version; `builtin` for the native
  parser compiled into the core; `null` when the route did not resolve a plugin), the
  node-kind skeleton (ancestors) and shape (nesting, depth ≤ 5, ≤ 8 children per node)
  around each changed region, the passes that removed or added draft changes (pass names,
  change types, node kinds; `suppressions: null` when the route records no trace), and
  zero-based line/byte-column offsets plus byte sizes of each region.
- **Privacy.** No source text: identifiers become placeholders (`id1`, `id2`, …) that are
  consistent within one report and never hashed; literals become their type and length
  (`string(12)`, `int`, `float`, `number`, `bool`, `null`); comments and docstrings are
  dropped. A node kind that is not lowercase grammar vocabulary, or that equals an
  identifier's text, is shown as `node`. No filename or path appears.
- **URL.** `issue_url` (= `incomplete.report_url`) is at most 8000 bytes. When the body does
  not fit, the URL carries a deterministic prefix of whole body lines plus a truncation note,
  `truncated` is `true`, and `body_markdown` stays complete so a binding can offer
  copy-to-clipboard.
- **Determinism.** The same inputs produce a byte-identical `report`.
- **Binding obligations.** Core never sends anything. A binding MUST show the user the
  `title` and `body_markdown` for preview and MUST require explicit user confirmation before
  opening `issue_url` (or copying the body). Never open the URL automatically, and never
  add filenames, source text or other local data to it.
- The CLI presentation (`render_cli_review`) shows an "Incomplete review" panel with
  `message` instead of "No changes detected".

### Guardrail check verdict (`guardrail_check_result`)

`api::guardrail_check_result(&Value)` and the C-ABI op of the same name (args `[request_json]`)
produce the one authoritative check verdict; bindings and the CLI render it and never recompute
counts or the gate. Request: exactly one of `"diffs": [SemanticDiff…]` (their
`guardrail_violations` are concatenated in order; an absent/null list counts as none) or
`"violations": [GuardrailViolation…]`, plus `"strict": bool` (required), `"checked_files":
int|null` (default: the number of diffs, or `null` for a bare violation list) and `"metadata":
object|null` (passed through). Result, exactly:

```json
{"violations": [...], "violation_count": 3, "immutable_count": 1, "important_count": 2,
 "checked_files": 4, "strict": true, "passed": false, "metadata": null}
```

**Gate rule:** under `strict`, ANY violation — `important` or `immutable` — fails the check;
a non-strict check is advisory and always passes. Unknown/missing severity, a missing or
non-boolean `strict`, both or neither of `diffs`/`violations`, and malformed counts/metadata
are errors. The native CLI's `guardrails --strict` exit status (2) uses the same function.

Native callers can use `routed_review::complete` for the same reconciliation as the C ABI.
It owns invariance suppression, generic/Markdown replacement, group indices, style decisions
and evidence. Suppression of non-equivalent sources does not manufacture equivalence evidence.
Partially retained meaningful groups describe only surviving changes; partially invalidated
relationship classifications are discarded. Explicit non-final index spaces are preserved.

Native Rust uses `symbol_index::{SymbolDefinition, ReferenceUsage, ReferencePosition, resolve_references}`.
Resolution matches exact qualified names, never chooses arbitrarily between multiple definitions,
and returns new DTOs without mutating the input usages. Python retains ordinary map lookup and
DTO transport; it does not decide uniqueness. Malformed ABI arrays return errors.

Native Rust hover selection is available as `lsp_enrich::collect_hover_targets`, shared
with `lsp_collect_hover_targets`. Returned columns are semantic UTF-8 byte columns.
Use `lsp_enrich::collect_utf16_hover_targets(tree, source)` or the
`lsp_collect_utf16_hover_targets` ABI handler (tree JSON string, source string) to
prepare UTF-16 protocol positions. Invalid source offsets and coordinate overflow
return explicit errors. Python advertises UTF-16 and uses this converted API.
Python selection failures must propagate; there is no Python semantic tree walker fallback.

## Patch reconstruction

Rust callers use `patch_source::reconstruct` and receive typed `PatchContent` with
`ReconstructionScope::{Complete, Excerpt}`. Only one text file is supported. Hunk
positions, context/deletions, line counts and final-newline boundaries are checked;
there is no fuzzy relocation. Binary and multi-file patches fail explicitly.

Without an original, ordinary modifications represent only contiguous visible
content, not a proven complete file. The result labels this as `excerpt` and carries
a warning. Unknown prefixes/internal gaps are errors; `require_complete=true`
rejects excerpts. Creation/deletion headers establish the empty opposite side.
Bindings must preserve this scope; Python exposes `is_partial` and warns on excerpts.

Filename inference follows the conventional `a/` old and `b/` new header pair; creation `b/` and deletion `a/` headers use the same convention. Plain headers naming the same `a/` or `b/` directory preserve it. Single-sided headers can be ambiguous: provide an explicit filename to preserve a literal prefix.

## Content routing

`content_type::detect_content_type` is a supported native Rust API, also exposed as `detect_content_type` through the C ABI. Rust owns the 8192-byte sampling window and routing decision. This is leading-sample classification, not complete-file UTF-8 validation: an incomplete final code point is tolerated because a caller may supply a truncated sample. Literal U+FFFD is valid text; explicit invalid UTF-8 and NUL classify as binary. Required engine failures propagate in Python. Native Git probes and Python streaming review use this shared detector.

## Ignore matching

Native callers compile `ignore_rules::IgnoreRules::new(&[IgnoreFile])`, then call
`is_ignored(path, is_dir)`. Each file supplies a repository-relative directory
(empty for root) and complete raw contents. No filesystem or global Git config is
consulted. Deeper rule files override parent matches, later lines take precedence,
and an excluded parent prevents child reinclusion. Matching is case-sensitive;
paths must be normalized relative POSIX paths, with no traversal/backslashes.

C ABI `match_ignore_rules` takes one JSON string with `files` (directory/content)
and `paths` (path/optional is_dir), returning booleans in input order. Invalid
rules/paths fail explicitly. Python `DiffIgnore` takes raw text and optional
`directory_rules` mapping; it retains only host file reads and DTO transport.
This replaces its old test-oriented pathspec-object constructor. Neither matcher
infers that a tracked deletion was caused by ignore rules.

## Parser candidate planning

Public `parser_routing::shortlist` accepts discovered `Candidate` descriptors and
`RoutingQuery`, returning source indices. Explicit plugin identities/aliases take
precedence over language filters, then filename matching. Unknown filenames leave
all candidates eligible. Generic is always last, followed by descending declared
priority and stable identity ties. Filenames accept either path separator and are
matched case-insensitively. `filename_candidates` omits the no-match fallback.
C ABI `parser_candidate_shortlist` takes JSON text with `entries` and `query`.
Empty/duplicate identities are errors. This plans candidates; it does not assert
successful component loading or capabilities. Python discovery has no declared
priority and supplies zero, independent of cache warmth. Native manifest routing
preserves its declared extension winner and now recognizes literal special names.
Full load/probe decisions, returned-claim validation and content-only ranking
remain tracked in #108.

The Dockerfile source fixture compares complete changes/groups/semantic flags
through native and Python APIs. Native omits optional null DTO fields; comparison
deserializes both through the public SemanticDiff schema without discarding any
change, node, group or evidence field.

### Cache metadata filtering

`cache_list_entries_filtered(path, ttl_days, max_mb, table, language, since,
before, min_size, max_size, limit, file_glob)` delegates to the public Rust
`cache_store::SqliteStore::list_entries_filtered` (also available through
`cache_registry`). The existing `cache_list_entries` boolean-window operation
remains available for compatibility; new bindings should use the filtered form.

Rust applies language/time/size filters, then matches either old or new diff
filename, then limits the result. Rows are ordered by `created_at DESC, key ASC`.
Globs use case-sensitive fnmatch syntax on every platform: `*`, `?`, character
classes and negation; separators are ordinary characters and backslash is not an
escape. This intentionally removes the old Windows Python case normalization.
Null/empty patterns and patterns on non-diff tables impose no filename filter.
Limits must be positive. Only metadata is read; matching streams until the
requested number of results is reached, without a finite over-fetch window.

### Content detection policy

`content_detection_plan(request_json)` and `content_detection_finish(input_json)`
delegate to supported Rust `content_detection::{plan, finish}`. The request carries
loaded `entries` (`plugin_id`, `grammar_id`, `languages`, `priority`), `content`,
optional `allowed_plugins` (see *Parser allowlist identifiers*), `candidates`, `plugin_id`, and
`preferred_plugins` (language to plugin ID). A plan returns eligible source indices
and a sample ending at the last complete UTF-8 character within 4096 bytes.

Hosts execute those probes and pass `{request, observations}` to finish. Each
observation has an index and a nullable language (null/empty means a genuine
decline). Duplicate, missing, ineligible or unsupported claims fail explicitly.
Probe errors, especially fuel exhaustion, must propagate from the host; they are
not declines. Generic results always rank last, followed by preferred plugin,
descending priority, language, grammar ID and plugin ID. Multiple plugins can
produce separate results for one language. Confidence is reciprocal rank rounded
to three decimal places with ties to even; it is not a measured probability.

The outcome contains ordered result DTOs and `not_found` for an explicit plugin
with no valid match. Python only invokes adapters, marshals DTOs and maps this
no-match outcome to its public exception. This slice does not migrate the existing
component loader or filename `detect_parser` failure/fallback policy (#108).

### Filename selection actions

`filename_selection_next({request, events})` accepts a JSON string and delegates
to public Rust `filename_selection::next_action`. Request fields are catalogue
`entries` (the candidate-planner DTO), `filename`, `content`, optional
`old_content` (defaults to an empty string), optional
`language_hint`/`plugin_id`/`allowed_plugins` (see *Parser allowlist identifiers*), and boolean `strict`.
For comparisons, hosts send the new source in `content` and previous source in
`old_content`. Rust probes the new source whenever it is nonempty (including
whitespace); only an exactly empty new source uses `old_content`. Single-source
callers may omit `old_content`. Bindings must not choose between the sources.
Hosts may bound each transported head to 2048 Unicode characters; Rust still
applies the authoritative UTF-8-safe 2048-byte bound below.
Actions are tagged `load`, `probe`, `selected`, `not_found`, or `failure`.
Hosts append exactly the requested `loaded`/`load_failed` or
`probed`/`probe_failed` event and replay. Loaded metadata includes grammar ID,
languages and priority. Failure actions identify the original host exception by
its event index. Probe samples are complete UTF-8 prefixes of at most 2048 bytes.

Hints precede filename candidates, other specific parsers, then generic. Strict
hints and explicit plugin requests never cross their boundary. The existing
non-explicit catalogue-hint alias of a generic component remains supported.
Metadata for all candidates in the current phase is loaded before ranking by
loaded priority and stable catalogue identity. A fuel/security denial during
that phase is terminal even when another candidate might succeed. Later phases
are not loaded after success. Ordinary load failures can fall through; probe
failures and undeclared claims cannot. Hosts must mark security/fuel load errors
terminal; Python uses typed exceptions, including `PluginSecurityError`, a
backward-compatible `PluginLoadError` subtype.

Events are bounded to twice the catalogue length and validated against the
requested action; duplicate/out-of-order events and events after an outcome fail.
The existing native bundled-manifest resolver has not yet adopted this protocol;
its integration and broader inventory loading remain tracked in #108.

`parser_availability(name, filename, system, machine)` returns an object with
`reason` (string or null) and `unavailable` (`[parser, reason]` or null). It delegates
to the public `parser_availability` Rust module. Hosts supply their OS/architecture;
both Python discovery and native bundled discovery apply this policy before
compiling components. This legacy four-argument operation always rejects
PowerShell on Windows ARM64, because it cannot know which Wasm bytes will load.

`parser_availability_with_digest(name, filename, system, machine, component_sha256)`
is the compatible **new** operation for a host that has staged a component.
The host performs permitted I/O to compute the lowercase or uppercase SHA-256
of the *actual staged Wasm bytes*; the Rust core alone evaluates the compatibility
policy and returns the same `reason`/`unavailable` shape. Only the component
built and tested with fuel-enabled Wasmtime 48.0.7 on real Windows ARM64 may
bypass the PowerShell exclusion: the registry #20 component from PowerShell parser
PR #9 (SHA-256 `b47cff15ae39f73f753c26babb7fabb393089c3dac1dc1d1ef43d1aba49e3ff0`,
parser CI #42994016353) and the earlier PR #5 component (SHA-256
`29a7be79db83f80627e98da8513a160dc047ea972fae4e3be07d741793a30c15`, CI #40707156445;
Core Rust API CI #40709266345). Missing, partial, unexpected and legacy
digests **fail closed**, and earlier four-argument clients remain excluded.
The digest is not a substitute for registry trust verification: hosts must
check the signed/pinned source and checksum and must not rely on untrusted
metadata as proof of bytes. Other OS/architecture availability is unchanged.
This Rust policy is a migration gate pending broader core #183 preflight
and final native/C ABI/Python semantic acceptance.

`parser_component_availability(wasm_filename, system, machine, component_sha256)`
is the raw-component loader guard. On Windows ARM64, a request for the standard
`powershell_parser.wasm` component with an empty digest returns a Rust-owned
restriction. A thin host can then hash the **actual bytes** and repeat the
operation; only the vetted PowerShell artifact on pinned Wasmtime 48.0.7
is allowed. Unrelated components
have a null `reason`, so no whole-catalogue hashing is needed. A missing C ABI
operation must **fail closed** rather than return a false success. The native
Wasmtime loader checks the exact bytes and compiles those same bytes, preventing
a check/reopen race on this guarded component. Other Wasm compiler aborts remain
in scope of core #183 and are not certified by this special-case guard.


### Human CLI presentation (Rust-owned)

`render_cli_review(request_json: str) -> str` formats an already-authoritative
`SemanticDiff` DTO using crates.io `rs-rich` 0.0.9. The public Rust entry point is
`cli_presentation::render_cli_review_impl`. The request contains `diff`, `width`
(16–300 columns), and `color` (boolean); hosts determine terminal width, honor
`NO_COLOR`, and choose stdout/stderr. The returned string includes its final newline.
No source parsing or semantic decisions occur here. Missing required diff flags or
malformed inputs return errors, never an empty success. Human summaries, guardrails,
change tables and explicit source-fallback warnings share this implementation;
JSON/patch/HTML/LLM/SARIF exports do not pass through it. Strings are literal text,
not rich markup, and terminal controls are escaped.


### Compatibility-only diff configuration (0.0.2)

The shared Rust diff configuration accepts the following compatibility fields only at
their existing defaults: `ignore_style=true`, `detect_refactorings=true`,
`leaf_text_diff=true`, `approx_move_threshold=0.4`, and `extra_trivia_types=[]`.
Omitting them preserves current engine behavior. Non-default values and wrong JSON
types produce a configuration error instead of being silently ignored. The camel-case
spellings are checked too; a default in one spelling cannot hide an override in another.
These fields do not yet provide configurable semantics. Possible support is tracked
in #220–#224; the contract inventory is #214. Bindings forward values and errors;
they must not implement substitute behavior.
