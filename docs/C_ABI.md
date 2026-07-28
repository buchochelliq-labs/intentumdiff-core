# The IntentDiff C ABI — the binding-author contract

The engine ships as a native shared library (`intentdiff_rust_core.{dll,so,dylib}`) exposing
**one stable language boundary**. Every binding — Python (ctypes), Go, Java (FFM), and any
future language — drives the identical surface. Bindings do zero functional work.

## The two exports

```c
char *intentdiff_call(const char *name, const char *args_json);
void  intentdiff_free(char *ptr);
```

- `name` — the engine function to invoke (UTF-8 C string).
- `args_json` — a **JSON array of positional arguments** (`"[]"` for none — never `null`).
- Returns a heap-allocated UTF-8 JSON **envelope** which the caller MUST release with
  `intentdiff_free`. A `NULL` return means an allocation/encoding failure only.
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

- Python: [`intentdiff-python`](https://github.com/buchochelliq-labs/intentdiff-python) (`src/intentdiff/rust_core.py` — `_CtypesBackend`)
- Go: [`intentdiff-go`](https://github.com/buchochelliq-labs/intentdiff-go)
- Java: [`intentdiff-java`](https://github.com/buchochelliq-labs/intentdiff-java)
