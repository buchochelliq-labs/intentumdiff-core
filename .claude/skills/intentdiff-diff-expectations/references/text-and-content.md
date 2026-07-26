# Text / prose / markdown / config expectations — "if you see X, expect Y"

Non-code files (markdown, plain text, `.gitignore`, config, licences, docs) are parsed by the
**generic parser** and are **line-oriented from the user's point of view**. The cardinal rule:
**a changed line is one clean change; the reviewer wants the line, not per-character or per-token
churn.** Markdown additionally gets section-aware presentation. All verified in
`tests/unit/test_generic_text_diff.py` + reproductions.

## Prose / plain-text lines (generic parser)

| If you see (X) | Expect (Y) | node_type / detail | MUST NOT |
|---|---|---|---|
| A word/phrase edited in a line (`The quick brown fox.` → `…dog.`) | **one** `MODIFICATION` of the whole line (old line → new line); the char highlight is in `text_diff` (`[-fox][+dog]` style) | `text_line`; `text_diff="The quick brown [-f][+d]o[-x][+g]."` | per-character `text_span` changes (`f`→`d`, `x`→`g`); >1 change for one edited line |
| Characters inserted mid-line (`alpha bravo` → `alpha brave new bravo`) | one `MODIFICATION`; `text_diff="alpha[+ brave new] bravo charlie"` | `text_line` | a separate `ADDITION` char-span |
| Characters deleted mid-line | one `MODIFICATION`; `text_diff="…[- brave new]…"` | `text_line` | a separate `DELETION` char-span |
| Insert **and** delete on the same line | one `MODIFICATION`; combined highlight in `text_diff` (`CON[-N]ECTION[+ COOL]`) | `text_line` | a `DELETION` + `ADDITION` pair for one line |
| A whole new line inserted (tail, middle, bullet) | one `ADDITION` `text_line` with the line as label; later lines do **not** shift/churn | `text_line`, label = the line text | modifications on unrelated shifted lines |
| A line removed | one `DELETION` `text_line` | `text_line` | char-span churn |
| Whitespace/blank-only line added | **ignored** (no change) | — | any surfaced change |
| Any generic-parser file with churn | token churn replaced by stable line/char spans; the `presentation.generic_text_diff` `NOISE_SUPPRESSED` group owns **`raw_change_indices=[]`** (count in `suppressed_count`) | — | the group claiming real changes; out-of-range indices |

## Markdown (generic parser + section-aware presentation)

| If you see (X) | Expect (Y) | rule_id / node | MUST NOT |
|---|---|---|---|
| A heading renamed, body unchanged (`# Old Title` → `# New Title`) | **one** `MODIFICATION` `markdown_section` (`# Old Title` → `# New Title`) under a `MEANINGFUL_CHANGE` group; the overlapping line change is stripped | `presentation.markdown_section_heading_rename` | a duplicate `text_line` MODIFICATION on the heading line |
| A section moved (same body, different position) | a section-move presentation, not add/delete of the whole section | `presentation.markdown_section_move` | add+delete leakage of the section body |
| A new section added (`## New` + a paragraph) | clean `text_line` `ADDITION`s for the new lines (blank lines suppressed) | `text_line` | token/char churn; spurious modifications on surrounding lines |
| A list item added (`- three`) | one `text_line` `ADDITION`, label `- three` | `text_line` | splitting the bullet into words |
| A word edited inside a paragraph | one `text_line` `MODIFICATION` (as prose above) | `text_line` | char-span churn |

## Config / data-as-text (.gitignore, ini, conf, dotenv, plain toml/yaml-as-text)

| If you see (X) | Expect (Y) | detail | MUST NOT |
|---|---|---|---|
| A new ignore/config line added (`.gitignore` `+ /.intentdiff`) | one `ADDITION` surfaced as a **first-class content change** (Content risk, not "Behavior"), not buried under the noise group | `text_line`; extension labels it Content/Config | the real insert nested under "Suppressed N noisy changes"; labelled "Behavior · New public API" |
| An ignore/config line edited (`/dist` → `/build`) | one `MODIFICATION` `text_line`, highlight in `text_diff` | `text_line`; `text_diff="/[+buil]d[-ist]"` | char-span churn |
| CRLF↔LF re-encoding + a real add | only the real add surfaces; the line-ending re-tokenization is suppressed (`generic_text_diff` group, `[]`) | — | every line reported as changed |

## Content class → risk (extension surfacing; see intentdiff-vscode `contentClass.ts`)

| If the file is (X) | Expect risk/label (Y) |
|---|---|
| code (has code node types / a real language) | Behavior (meaningful) / Internal (refactor/move) |
| docs (markdown/rst/mdx/asciidoc) | **Content** — "Documentation", never "Behavior"; no "New public API" phrasing |
| config (gitignore/ini/toml/dotenv/conf/editorconfig) | **Content** — "Configuration entry change" |
| data (json/yaml/xml/csv) | **Content** — "Data/config value change" |
| text/generic | **Content** — "Text content change" |

## Why these matter (the recurring intents)
- **Line, not characters.** For prose/config the meaningful unit is the line; char-level detail
  belongs in `text_diff`, not as separate changes. (The historical `text_span` per-opcode shape
  was noise and is retired.)
- **No duplication across presentation passes.** When a section-aware pass (markdown heading
  rename/move) surfaces a change, the overlapping generic line change must be removed, not shown
  twice.
- **Non-code is Content, not Behavior.** A `.gitignore`/markdown edit is a content change; it must
  never read as runtime "Behavior" or "New public API", and a genuine insert must never be buried
  as suppressed noise (see the change-group index-space contract).

## Prose line reorder (fixed 2026-07-06, issue #14)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Adjacent line swap | lines 1↔2 swapped, rest unchanged | exact_total = 0 — identical content whose position changed nets out (`_net_out_relocated_generic_lines`); prose has no execution order. Non-identical lines and unpaired adds/deletes pass through untouched. |

## Markdown section move (fixed 2026-07-06, issue #15)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Section swap | `## A` and `## B` (with bodies) swap order | exact_total = 1 — ONE section MOVE (LIS insertion-shift discrimination, the #12/#32 rule applied to section order); the body travels with the section (relocated identical lines net out per #14); blank-separator churn suppressed (deleted blank lines skipped symmetrically with added ones). |

## Generic-text stage: PORTED TO RUST (2026-07-06, issue #35)

The generic text review (line diff, relocated-line netting #14, blank symmetry #15, inline
char detail, and the `presentation.generic_text_diff` suppression AUDIT group) now runs in
the Rust core (`generic_text_review_json`, strangler pattern per `apply_invariances_json`).
Python (`_generic_text_changes` et al.) remains ONLY as the size-cap fallback (LCS table
> 4M cells). All rows in this file are enforced by BOTH layers: the Python scenario suites
and four Rust `#[cfg(test)]` pins (added-line+audit-group, reorder-nets-to-zero,
one-modification-with-inline-detail, blank-deletion symmetry).

## Markdown section stage: PORTED TO RUST (2026-07-06, issue #36)

Section moves (LIS insertion-shift, swap = ONE move) and heading renames (unique body-hash
identity) now run in the Rust core (`markdown_section_review_json`); Python keeps only the
change-list filtering + fallback. Pinned in BOTH layers: the markdown scenario suite +
heading-rename contract in test_generic_text_diff.py, and two Rust `#[cfg(test)]` pins.

## .gitignore / ignore-family (dedicated parser, issue #43, 2026-07-07)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A `.gitignore` (or `.dockerignore`/`.npmignore`/…) edit | language `gitignore`, NOT `generic` — pattern/comment/negation nodes, blank lines dropped structurally | `test_gitignore_parser.py`; crate `intentdiff-gitignore-parser` |
| A single pattern added | one `ADDITION pattern '<text>'`, NO `presentation.generic_text_diff` NOISE_SUPPRESSED group, no "ungrouped raw evidence" — the extension promotes it to a first-class Meaningful change | `test_added_pattern_is_one_clean_change_with_no_noise_group` |
| Blank lines added/removed between patterns | zero changes (spacing is invisible — the generic-text token-churn defect this parser removes) | `test_blank_line_churn_is_invisible` |
| A negation (`!pattern`) | node_type `negated_pattern`, distinct from `pattern`, so un-negating reads as a real edit not a delete+add | `test_negation_is_distinct_from_a_plain_pattern` |

**Detection wiring for a new filename-only parser:** the parser's own wasm `detect_language`
is not enough — the catalog shortlist (`_entry_matches_filename`) reads the STATIC
`language_metadata.py` maps (default filename, extensions, names) without loading wasm. A new
language must be added there too, or only filenames literally containing the language id
(`.gitignore`) route while siblings (`.dockerignore`) fall to generic.

## .gitignore intent wording (engine-owned, issue #58, 2026-07-07)

| If you see (X) | Expect `Change.description` (Y) | enforced by |
|---|---|---|
| ADDITION of a `pattern` | `Adds an ignore rule for <text>` | `test_engine_emits_human_intent_descriptions`; rust `ignore_intent_description` |
| DELETION of a `pattern` | `Stops ignoring <text>` | same |
| ADDITION of a `negated_pattern` | `Adds an exception for <bare> (no longer ignored)` | same |
| Any change on a NON-ignore node | structural description untouched (`Insert -> …`) | rust pin `ignore_intent_descriptions_read_as_human_review` |

The wording is produced in the Rust core (`edit_op_to_draft` → `ignore_intent_description`),
so it reaches every frontend via `Change.description`. gitignore is routed through the Rust
finalize path (RUST_FINALIZE_LANGUAGES) precisely so python `presentation.py` does not
overwrite the description with the raw `Insert -> pattern(...)` form. The "untracks N files"
impact needs the working tree and stays a frontend concern.
