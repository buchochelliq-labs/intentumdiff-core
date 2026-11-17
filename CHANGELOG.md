# Changelog

## Unreleased — v0.0.3 correctness foundation

- Every served review now carries `completeness` (`complete` | `equivalent` | `incomplete`)
  and `incomplete` (core #252). Differing sources whose drafts were all suppressed without an
  equivalence proof are `incomplete`, never a clean review: the DTO keeps `changes=[]`, adds
  changed byte/line ranges and a core-authored report-a-bug `message` with a stable `reason`
  and `report_url`. Identical input is `complete`; proved style-only input is `equivalent`.
  Binding authors: render `incomplete.message` verbatim (see `docs/C_ABI.md`).
- An `incomplete` review now carries `incomplete.report = {title, body_markdown, truncated,
  issue_url, outline}`: a core-built, sanitised repro outline (core/language/plugin
  versions, node-kind skeletons and shapes, the passes that removed drafts, line/column
  offsets and byte sizes; identifiers as `id1…`, literals as type and length, comments
  dropped). `report_url` is the pre-filled GitHub issue URL, capped at 8000 bytes with
  deterministic truncation. Bindings MUST preview it and require explicit confirmation
  before opening the URL; core never sends anything.
- Member reorders inside JSON objects, YAML mappings and TOML tables are proved equivalent
  (`metadata.equivalence_proof = keyed_data.unordered_mapping_member_order`); duplicate or
  escaped keys, YAML anchors/aliases/tags and TOML arrays of tables decline the proof.
- The language assessment checks the completeness state; cases may pin
  `expected.completeness`.
- Add a source-pinned six-language external pilot, explicit admission ledger for
  all 74 routed IDs, independent control expectations and machine-readable results.
- Add an offline Rust public-API benchmark gate and exact installed-binding
  acceptance, separating semantic accuracy from parity and infrastructure failures.
- Consume the verified C/C++ component preserving character-literal payloads,
  prefixes, escapes and byte spans. Wider 0.0.3 acceptance remains open.

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
