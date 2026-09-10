# Changelog

## Unreleased — v0.0.2 release candidate

- Preserve callable rename identity alongside independent body edits and statement
  reorders, including insertion of a sibling helper. Require unambiguous body and
  enclosing-scope evidence; keep meaningful body groups separate from rename groups.
- Add native and routed regression evidence for Python, plus public-wrapper
  JavaScript/TypeScript checks.
- Prefer exact source/body evidence for callable continuity, reject ambiguous positional
  fallbacks, and preserve Python decorator order through sibling insertion and formatting.
- Recognize bounded Python expression-to-helper extraction while retaining unrelated edits.
- Move active Python literal enrichment and tree-equivalence evaluation behind the C ABI;
  preserve UTF-8 source spans and literal whitespace. Limit CSS color equivalence to values.

- Preserve exact callable renames across parser signature shapes and opaque declarations,
  while retaining ambiguity protections. Keep nested CSS custom-property tokens meaningful.

## v0.1.0 — 2026-07-26

Initial import from the IntentumDiff monorepo (files-only; the monorepo remains the archive of
record). The complete engine: semantic diff pipeline across 69 languages, the stable C ABI
(`intentumdiff_call`), the native `intentumdiff` CLI, the index engine, and the four renderer
components. Engine pin suite 203/203.
