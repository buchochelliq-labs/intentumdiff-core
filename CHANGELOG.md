# Changelog

## Unreleased — v0.0.2 release candidate

- Preserve callable rename identity alongside independent body edits and statement
  reorders, including insertion of a sibling helper. Require unambiguous body and
  enclosing-scope evidence; keep meaningful body groups separate from rename groups.
- Add native and routed regression evidence for Python, plus public-wrapper
  JavaScript/TypeScript checks. Helper extraction classification remains outstanding.

## v0.1.0 — 2026-07-26

Initial import from the IntentumDiff monorepo (files-only; the monorepo remains the archive of
record). The complete engine: semantic diff pipeline across 69 languages, the stable C ABI
(`intentumdiff_call`), the native `intentumdiff` CLI, the index engine, and the four renderer
components. Engine pin suite 203/203.
