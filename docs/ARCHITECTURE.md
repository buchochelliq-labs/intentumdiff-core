# IntentumDiff engine architecture

The architectural target is a **complete shared backend**, with every product surface a thin
binding over the [C ABI](C_ABI.md). The Python-first audit found active semantic postprocessing
in the Python shell; its removal is tracked in
[Python#54](https://github.com/buchochelliq-labs/intentumdiff-python/issues/54).
Overlapping implementations must be compared against explicit semantic expectations before
migration; neither implementation is automatically the correctness oracle.

## Pipeline

```
source pair ──► parse ──► SemanticNode trees ──► match ──► classify ──► finalize ──► SemanticDiff
```

1. **Parse.** Python is served by the certified native batch (tree-sitter in-process,
   `convert_cst`); every other language parses through its Wasm component parser (WASI p2,
   Component Model) loaded by the wasmtime host with fuel + memory limits.
2. **SemanticNode trees.** Parsers emit deterministic trees: `id` (dotted path), `node_type`,
   `label`, 0-based positions, structural hashes, and privacy-safe `NodeFacts` (counts/enums/
   flags — never source text) computed natively.
3. **Match.** Hash-based subtree matching, entity anchoring, rename/move promotion, cross-file
   symbol/reference matching (via `index-engine-lib`, linked natively).
4. **Classify.** Each change becomes MEANINGFUL / REFACTORING / MOVED / IGNORED_STYLE / NOISE
   with a derived confidence and an intent description; invariance rules suppress
   known-equivalent rewrites; guardrail rules evaluate protected paths. Authoritative catalogue
   consolidation is tracked in [#39](https://github.com/buchochelliq-labs/intentumdiff-core/issues/39).
5. **Finalize.** Presentation passes (grouping, compaction, reorder suppression, per-language
   statement/keyed/resource profiles) produce the public `SemanticDiff`.

## Callable rename continuity

Existing entity identities are matched first. An unmatched callable can then establish
continuity across a rename when its signature is unchanged, its full enclosing named scope
agrees, its bodies have comparable sizes and shared concrete content, and the candidate is
unique in both directions. Similarity is symmetric; containment in an extracted helper alone
does not establish a rename. Ambiguous cases remain unpaired.

The rename changes the declaration name only. Body modifications remain separate changes;
the older safety guard on collapsing whole deletion/addition subtrees remains in place.
Insertion shifts use the established rename identity when distinguishing shifts from moves.
See the [reproduction and CLI evidence](evidence/rename-body-edit/README.md).

## Crate topology

| Crate | Role |
|---|---|
| `crates/rust-core-host` | the engine (this document); builds as **cdylib** (the C ABI) + **rlib** (in-process Rust consumers) |
| `crates/cli` | the native `intentumdiff` CLI — links the engine rlib, standalone `[patch]`/`[profile]` |
| `crates/index-engine-lib` | symbol/reference tables + cross-file diff, linked natively AND into the index Wasm component |
| `crates/index-engine`, `crates/*-renderer` | Wasm components (workspace members; SDK git dep) |
| `crates/patches` | vendored `[patch.crates-io]` crates (build-script stabilization) |

The root workspace holds the Wasm members; the engine and CLI are workspace-**excluded** so
their own `[patch]`/`[profile]` tables stay authoritative (cargo only honors patch tables in a
build's top-level manifest).

## Subsystems in the engine

Config (intentumdiff.yaml), the SQLite parse/diff cache + analytics store (stateless C-ABI
surfaces over warm path-keyed registries), the registry client validators (#88 controls),
git/VCS readers (git/hg/svn/p4 CLIs — no libgit2), the perceptual image diff, and the
live-diff/review/LSP protocol handlers consumed by
[intentumdiff-live-server](https://github.com/buchochelliq-labs/intentumdiff-live-server) and
[intentumdiff-lsp](https://github.com/buchochelliq-labs/intentumdiff-lsp).

## Test tiers

- **Tier A** — the engine pin suite (`cargo test` in `crates/rust-core-host`): 178 always-on
  pins + 25 `tier-c-wasm` certification tests that load staged parser components (default-on
  feature; CI runs `--no-default-features` until cross-repo component provisioning is wired;
  point `INTENTUMDIFF_TEST_WASM_DIR` at a dir of built components to run them anywhere).
- Component and binding tiers live in their own repos; the registry pins trusted artifacts.
