# IntentDiff

**Semantic code review: detect intent, moves, refactorings, and style changes.**

IntentDiff diffs code by *meaning*, not lines. It parses both sides into semantic trees,
matches and classifies every change (meaningful / refactoring / moved / style-only /
noise), derives risk, and explains *why* — across 69 languages and formats, from Python
and TypeScript to Terraform, SQL dialects, and Markdown.

This repo is the **engine** — the complete shared backend. Every product surface (the
Python package, the VS Code extension, the native CLI, future Go/Java bindings) is a thin
binding over the code here: bindings do zero functional work.

## What lives here

```
crates/rust-core-host/   the engine: CST -> SemanticNode conversion, tree diff, NodeFacts,
                         finalize/review, guardrails, cross-file, config, cache, registry
                         client, git/VCS readers, perceptual asset diff. Builds as a cdylib
                         exposing the stable C ABI (intentdiff_call) + an rlib for
                         in-process Rust consumers.
crates/cli/              the native `intentdiff` CLI (clap) — drives the engine in-process.
crates/index-engine/     the symbol/reference indexing Wasm component (+ index-engine-lib,
                         also linked natively by the engine for cross-file diffs).
crates/*-renderer/       the terminal / patch / html / llm renderer Wasm components.
crates/patches/          vendored [patch.crates-io] crates the build graph needs.
```

Parsers are separate Wasm-component repos (one per language) built on
[intentdiff-plugin-sdk](https://github.com/buchochelliq-labs/intentdiff-plugin-sdk) and
registered in [intentdiff-registry](https://github.com/buchochelliq-labs/intentdiff-registry)
— the root of trust that pins every official component to a commit + SHA-256 checksums.

## The language boundary

One stable C ABI: `intentdiff_call(name, args_json) -> json envelope` (+ `intentdiff_free`).
Any language binds it — the Python package does so via ctypes; the CLI and the native
live-server link the engine directly as Rust. The engine has no Python (or any host
language) dependency.

## Build

```bash
# the engine (native)
cd crates/rust-core-host && cargo test          # engine pins
cargo build --release                            # the cdylib + rlib

# the CLI
cd crates/cli && cargo build --release           # -> target/release/intentdiff

# the Wasm components (index engine + renderers)
cargo build --release --target wasm32-wasip2     # from the repo root workspace
```

Toolchain: Rust **1.93.0** (pinned in CI); `wasm32-wasip2` for components.

## Provenance

Migrated files-only (no history) from the IntentDiff monorepo
(`buchochelliq-labs/intentdiff`), which remains the archive of record. The
`.claude/skills/` directory carries the engine development skills.

License: MIT.
