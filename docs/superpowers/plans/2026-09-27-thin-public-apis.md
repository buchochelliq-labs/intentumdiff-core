# Thin public APIs migration plan

Spec: ../specs/2026-09-27-thin-public-apis.md

## Global constraints
Two supported public APIs, one Rust implementation; future bindings use the same ABI. Preserve external-runtime packaging. No Python semantic fallback. Native Rust must not depend on Python. Existing features migrate without silently losing source evidence. New Rust functions are callable directly and through ABI handlers. User explicitly authorized implementation and independent review.

## Task 1: Shared lifecycle finalization (#99)
Expose typed Rust lifecycle finalization and an ABI adapter. Replace Python group filtering/flag calculation with DTO delegation. Corpus: modified/add/delete, empty files, style-only and meaningful groups. Test missing/invalid lifecycle input and native/ABI result equality. Run targeted Rust and Python tests before integration.

## Task 2: Shared host-utils (#98)
Move guest callback computation to Rust. Specify root trivia as JSON null, support CST and semantic-tree spellings, bound input/trivia/depth, and make malformed input explicit. Python callbacks marshal arguments only. Add native/ABI/Python parity and actual guest coverage where available.

## Task 3: Schema identities (#97)
Move pure provider detection, declared-schema extraction, user-profile matching/validation and identity derivation into public Rust operations. Keep fetch/filesystem/command execution in hosts. Thread configured identities into native review as well as Python enrichment. Test precedence, custom keyed arrays, reorder plus meaningful body edits, Unicode and descriptor errors against explicit expected output.

## Task 4: Markdown reconciliation (#55)
One shared Rust operation returns complete presentation changes/groups. Remove Python section matching/hash/LIS and result filtering. Tests must prove duplicate headings do not erase unrelated changes; heading rename plus body edit retains both; insertion produces no spurious move; errors fail loudly.

## Task 5: Compile context (#100)
Rust interprets supplied database entries and path context, flags and deterministic selection. Host reads files. Exact matches win; ambiguous/unrelated basename cannot be silently selected. Cover quoted args, relative directories, platform syntax and malformed entries.

## Task 6: Integration and independent review (#101, Python57)
Extend shared corpus with hand-reviewed expectations and generate actual diff artifacts from both public APIs. Run core suite and Python unit suite with real core and provisioned parsers; record any pre-existing unsupported cases explicitly. Reviewer checks public API usability, native/ABI/Python consistency and actual diffs. Fix findings with regression tests. Publish feature PRs against current release candidate, retaining existing reviewed fixes as dependencies.

## Review focus
Schema custom identities must reach native finalization; host error contracts must not hide malformed trees; Markdown reconciliation must not drop unrelated groups or source changes; lifecycle must distinguish empty modified files from add/delete; compile context must not choose another source file by basename.
