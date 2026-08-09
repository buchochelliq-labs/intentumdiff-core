# Core behaviors (cross-cutting) — "if you see X, expect Y"

The language-agnostic rules the engine already gets right (green tests), as input→expectation
tables. Read a row as: **given this input shape (X), the engine must produce this output (Y)**,
under this `rule_id`. These are part of the oracle so a port doesn't regress them. Per-language
specifics are in [failing-scenarios.md](failing-scenarios.md).

## Equivalence / ignored-style (nothing meaningful changed)

| If you see (X) | Expect (Y) | rule_id / group | enforced by |
|---|---|---|---|
| Only comments/whitespace/layout changed (filtered old/new trees hash-equal) | `is_style_only=True`, `changes=[]`, one `IGNORED_STYLE` group with source-span evidence (`evidence_depth="source_span"`, old/new spans). `metadata.index_space="style_only_shortcut"` (a source-span space, **not** change indices — leave untouched in reindex) | `generic.style_only_shortcut.source_equivalence` | `test_invariances.py` |
| Numeric literal reformatted, same value: `1_000` → `1000` | `IGNORED_STYLE`, `canonical_old/new="int(1000)"`; **no** meaningful change | `core.integer_literal.canonical_value.safe` | `test_invariances.py` |
| Numeric literal changed value: `1` → `2` | a **MEANINGFUL** change (values differ) — never style-only | — | `docs/BACKLOG.md` (SQL parser bug drops values → false style-only) |
| String quote style only: `'x'` → `"x"` | ignored (same value) | string-quote equivalence | `test_invariances.py` |
| CSS color re-expressed: `#00f` / `rgb(0,0,255)` / `blue` | ignored (equal color); stays individually inspectable | css color equivalence | `test_invariances.py` |
| CSS color changed: `blue` → `red` | a **MEANINGFUL** change (different color) | — | `test_invariances.py` |

## Reorder / move

| If you see (X) | Expect (Y) | rule_id / group | enforced by |
|---|---|---|---|
| Sibling reorder of a **non-entity** node (expression/scaffold) | change removed from `changes`; one `NOISE_SUPPRESSED` group; **`raw_change_indices=[]`** (owns no surviving change), `suppressed_count` in metadata | `refinement.suppress_low_signal_reorders` | `test_moves.py` |
| Reorder of a **named** function/class/module | promoted to `MOVE` / `MOVED_CODE` group, carrying node ids | `refinement.entity_reorder_to_moved_code` | `test_moves.py` |
| An entity shifted only because something was **inserted above** it | **NOT** a `MOVE`/`REORDER` | — | per-language "no false moves" rows |
| A genuine relocation of an entity | exactly one `MOVE` with the entity name; **no** add/delete/reorder leakage | — | `test_snippet_gap_regressions.py` (JS Moved Code) |

## Refactoring

| If you see (X) | Expect (Y) | rule_id / kind | enforced by |
|---|---|---|---|
| An identifier renamed | one `REFACTORING` with a `RENAME_*` kind — **not** add+delete | `RENAME_SYMBOL`/`_VARIABLE`/… | `test_moves.py` / snippet tests |
| Parameters renamed `a→x`, `b→y` | one rename each, **scoped to the signature** — **not** `CHANGE_SIGNATURE` `(function)`/`(anonymous)` churn, not identifier `ADDITION`s | — | dart/elixir/r/lua rows |

## Structural invariant — change-group index space (all languages)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| Any exposed `change_group` | its `raw_change_indices` are **valid final `changes` indices owned by node identity, or `[]`** (producers carry node ids or emit empty; consumers range-check) | `test_reindex_groups.py`; intentumdiff-engine → index-space-contract |
| A group that suppressed changes now absent from the final list (reorder-suppress, `generic_text_diff`) | `raw_change_indices=[]`, count preserved in `suppressed_count` | `test_reindex_groups.py` |

## Data / keyed languages

| If you see (X) | Expect (Y) | rule_id | enforced by |
|---|---|---|---|
| Scalars inserted into a JSON array | only `ADDITION`s for the inserted values; later shifted values are **not** `MODIFICATION`s | `presentation.repair_shifted_array_scalar_*` OR reorder-suppress | `test_json_presentation.py` |
| Keyed/resource/query edit (json key, puppet type+title, SQL field/clause) | matched by **semantic key**, not position | keyed/resource/query profiles | snippet tests |

## Non-code content

| If you see (X) | Expect (Y) | rule_id / group | enforced by |
|---|---|---|---|
| Generic-parser token churn (e.g. `.gitignore` edit) | replaced with stable line/char spans; `NOISE_SUPPRESSED` group owns `[]`; the real edit surfaces as a first-class **content** change (not "Behavior", not buried noise) | `presentation.generic_text_diff` | intentumdiff-vscode content classes; `test_reindex_groups.py` |

## Guardrails

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A protected semantic path changes in a keyed/resource config language | a `GuardrailViolation` (`rule_id`, severity `important`/`immutable`, `semantic_path`, old/new value), **key-based** not line-based | intentumdiff-guardrails; guardrail tests |

## Hardening invariants (oracle-free, issue #45)

| If we see X | Then expect Y |
|---|---|
| Any corpus snippet parsed | Every manifest-listed token visible in semantic labels (content-visibility — the kind-drift detector; caught-classes: #16 #21 #23 #41) |
| Any content mutation (bump literal, edit string, append statement) | NEVER style-only, NEVER zero changes (mutation non-equivalence — the #41 detector) |
| Zero final changes after suppression | Must NOT be presented as style equivalence — style-only comes ONLY from normalized-tree equality |

Harness: `tests/unit/test_corpus_invariants.py` over `tests/fixtures/corpus/<lang>/` (source +
`.expect.json` manifest). Tier 1 = python/perl/dart/sql/delphi. Growing per parser; Tier 2 =
Exercism cross-language snippets; Tier 3 = real bugfix pairs (BugsInPy + self-mined git history).

## Labels & parser-hash contracts (2026-07-07)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A literal's value in a change-group label | the label is **source-exact including quotes** (120-cap), e.g. `"old"` not `old` — assert `"old" in label`, never `label == "old"` | `test_competitor_issue_regressions.py` (csharp), `test_intent_truth_fuel.py` (powershell `"ORDER-$Id"`) |
| A hand-written (non-tree-sitter) parser builds parents before children (frame/stack pattern) | the parent's `structural_hash` is recomputed **after** children attach — a child-blind hash makes field additions hash style-only (graphql playground bug) | graphql crate test `adding_a_field_changes_parent_and_root_hashes`; `test_supported_language_example_contract[graphql]` |
| Wasm fuel near hotspot thresholds | thresholds carry ~30% headroom over the measured worst legitimate grammar; whole-binary LTO makes fuel swing ±10-15% across rebuilds from ANY crate change — never calibrate to <10% margin | `_FUEL_HOTSPOT_*` in `differ.py`; `test_fuel_truth_hardening.py` |

## Finalize-pass composition (issue #57 pilot, 2026-07-07)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| An empty body gains its first statements (go `func f() {}` -> real body) | after descendant-noise collapse the sole-carrier `block` ADDITION **survives** container suppression — a container drops only when every content leaf is matched or carried by another surviving draft | Rust pin `container_noise_keeps_sole_carrier_and_drops_matched_rewrap`; `test_scenarios_go.py::test_trivial_body_to_real_body` |
| A same-label MODIFICATION with no id-stable leaf delta covers a DELETE+ADD pair (go error-wrapping) | the no-delta filter runs BEFORE pairings suppression, so the dying modification cannot swallow the pair — the edit surfaces as DELETE+ADD | Rust pin `dying_no_delta_modification_cannot_swallow_its_add_delete_pair`; `test_wild_truthiness_regressions.py::test_go_error_wrapping_change_must_surface` |

## Moved-entity edit recovery + member anchoring (csharp pilot, 2026-07-07)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| An edit INSIDE an entity moved across nesting levels (csharp `namespace X {...}` -> file-scoped: `Bar() => 1` becomes `=> 2`) | the edit is recovered by positional leaf alignment (ids renumber wholesale — id-based pairing is blind) and surfaces anchored at the MEMBER level (`method_declaration 'Bar'`), not as a bare `integer_literal('2')` | `test_wild_truthiness_regressions.py::test_csharp_block_to_file_scoped_namespace_does_not_emit_brace_noise`; rust `promote_label_updates_inside_moved_entities_drafts` fallback + `suppress_parent_modifications_drafts` member preference |
| A same-label MODIFICATION whose subtrees share NO leaf ids (fully renumbered) | "no delta" must be decided by positional leaf comparison, not id lookup — equal-shape subtrees with a same-type label delta are a REAL edit | `subtree_has_leaf_label_delta` renumbered-subtree fallback |
| Rust passes needing the entity-container vocabulary | `is_entity_container_type` (port of python `_ENTITY_CONTAINER_TYPES`) — `is_named_entity_type` is the MATCHER's list and was missing `class_declaration` etc.; merging the two vocabularies is #49 | csharp pilot; #49 |

## Annotation-added → CHANGE_SIGNATURE (java/csharp pilot, 2026-07-07)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| An annotation/attribute added to a matched method (`@Override`, `[Attribute]`) | a member-level `REFACTORING(CHANGE_SIGNATURE)` on the method, NOT bare `modifiers`/`marker_annotation` child drafts; a concurrent body edit still surfaces | `test_java_override_and_import_reorder_remain_reviewable_not_noisy`; rust pin `added_annotation_promotes_method_to_change_signature` |
| A tree-based signature-change check | narrowed to the annotation/modifier/attribute signal so it never restates python parameter-change behavior, and yields to the source-based `promote_python_signature_changes_from_sources` via the REFACTORING pair guard | `promote_signature_changes_from_annotations_drafts` |

## Moves escaping deleted containers (issue #57 Root A, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A statement relocates OUT of a container that collapses/deletes (csharp `name = "guest"` leaving `if (name==null){...}` → `name ??= "guest"`) | the matcher pairs it, the edit script emits `MOVE` even though the OLD PARENT was deleted, and descendant-noise does NOT swallow it → `[DELETION if_statement, MOVE expression_statement]` | rust pins `move_out_of_a_deleted_container_survives_descendant_noise`; `test_semanticdiff_competitive_scenarios[csharp-null-coalescing-guard]` |
| A matched node whose old parent is UNMATCHED (deleted) | is a MOVE — the edit script's `same_container` test treats a deleted old parent as a container change, not a bail | `generate_edit_script_with_diagnostics_indexed` |
| Descendant-noise suppression | splits roots by type: a DELETION covers only deleted/moved descendants; a MOVE is suppressed only by a MOVED ancestor (rode along) or moved/added new-side ancestor — never by a mere deletion of its old container | `suppress_descendant_noise_drafts` |

## csharp formatting-equivalence IGNORED_STYLE (issue #57 Root B, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A csharp diff whose surviving MODIFICATIONs carry a formatting anchor (an `order_by_clause` modification, or a label with `{0}` / `Name:`) | in addition to the precise MEANINGFUL_CHANGE groups, an `IGNORED_STYLE` group `csharp.formatting.initializer_query_output_wrapping_equivalence` spanning the modifications' labels/ids — recording that initializer/LINQ/output wrapper churn was compacted. `provenance: "suppression"` (NOT an equivalence proof — must not, alone, make a zero-change diff style-only, #51) | rust pin `csharp_formatting_anchor_emits_ignored_style_group`; `test_csharp_style_changes_match_semanticdiff_signature`, `test_stage5_csharp_style_keeps_three_meaningful_changes_only` |

Ported from python `presentation.py` `_style_context_groups_from_final_changes` +
`_style_groups_from_suppression` (`_STYLE_RULE_BY_LANGUAGE` / `_STYLE_RULE_METADATA`) into the
Rust finalize (`formatting_equivalence_group_drafts`). python/javascript share the family but
are not finalize-routed yet.

## Routed zero-change source-equivalence group (issue #57, xml flip, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A finalize-routed diff that nets **zero** surviving changes while `old_source != new_source` (xml attribute reorder `<a x="1" y="2"/>` → `<a y="2" x="1"/>`) | the same `IGNORED_STYLE` group the default path attaches post-diff — `generic.style_only_shortcut.source_equivalence`, source-span evidence, `is_style_only=True` — NOT an empty change list with no record of the suppression | `differ.py` stage-9 routed block (`build_style_only_evidence` mirror); `test_wild_truthiness_regressions.py::test_xml_attribute_reorder_is_not_a_semantic_change` |

The routed finalize short-circuits before the default path's post-diff style stage, so `differ.py`
mirrors that one stage in the routed block. It fires for **any** routed language whose diff nets to
zero with differing sources, not just xml — keep it language-agnostic. `differ.py` stays thin: this
is the same `build_style_only_evidence` the default path already calls, invoked at parity, not new
processing logic.

## Haskell signature/function companion churn (issue #57 haskell flip, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A whole Haskell routine added or removed — the parser emits a sibling `signature` (`f :: ...`) **and** `function` (`f x = ...`) both labelled `f` | keep only the `function` ADDITION/DELETION; fold the sibling `signature` change away as scaffold with a `NOISE_SUPPRESSED` group `presentation.haskell.suppress_signature_function_sibling_churn`. A `signature` whose `function` is NOT also added/removed survives. | rust pin `haskell_signature_addition_folds_into_function_addition`; `test_snippet_gap_regressions.py::test_haskell_signature_and_function_addition_is_compact` |

Ported from python `presentation.py::_suppress_haskell_signature_function_sibling_churn`.
**Language-gated to haskell** — invoked from `finalize_review_json` (like
`formatting_equivalence_group_drafts`), mirroring python's `elif language == "haskell"`
dispatch, so the core finalize stays language-agnostic. The suppression pairs a `signature`
change to a same-label `function` change of the SAME direction (ADD↔ADD, DELETE↔DELETE).

## Parameter renames + dart scaffold (issue #21/#57, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A parameter identifier MODIFICATION whose ancestor is a param-list container of ANY grammar spelling (`formal_parameter_list`, `parameter_clause`, `declargs`, …) | promote it to `REFACTORING RENAME_VARIABLE` — the finalize promoter uses `is_parameter_list_type` (mirrors python `anchors.py::_PARAM_LIST_TYPES`), not the bare `"parameters"` type | rust pin `dart_param_container_is_recognised_across_grammar_spellings` |
| A routed dart routine addition leaking a sibling `function_body`/`block`/`return_statement` add/delete around the anchored `function_signature` | suppress the body scaffold, keep the signature; `NOISE_SUPPRESSED` group `presentation.dart.suppress_signature_body_scaffold_churn` (language-gated to dart in `finalize_review_json`) | rust pin `dart_signature_body_scaffold_churn_is_suppressed` |

Ported from python `presentation.py::_suppress_dart_signature_body_scaffold_churn`. **dart is NOT
yet routed** — its param rename `add(a,b)→add(x,y)` needs scoped-parameter-rename detection
(pair a `formal_parameter` delete/add with its body-identifier modification), which is not yet in
the finalize (see #21, overlaps #39). The dart scaffold pass is dormant until that lands.

## Resource-profile matching — puppet (issue #39, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A puppet diff where a resource attribute's VALUE changed (`message => 'Hello'` → `message => $message`) | an attribute-level `MODIFICATION` anchored on the same-key attribute (`attribute 'message' → attribute 'message'`), NOT a child `string → variable` cross-pair swallowing the edit | rust `augment_resource_profile_changes_drafts`; `test_puppet_playground_uses_resource_titles_and_parameter_identities` |
| A puppet class that gains parameters (parser emits empty-labelled `parameter` under a `parameter_list` scaffold) | individual `ADDITION`s labelled by the parameter's `variable` child identity (`message`, `target`), via label enrichment + keyed review-container surfacing — not a bare `parameter_list` addition | rust `enrich_resource_profile_labels` + `augment_resource_profile_changes_drafts`; rust pin `puppet_new_parameter_addition_uses_enriched_variable_label` |
| A puppet resource/attribute node | keyed by identity (resource type+title, attribute name, scoped by enclosing class) so the matcher pairs by identity, not position | rust `resource_profile_key`/`puppet_key`; pin `puppet_resource_and_attribute_keys_are_identity_based` |

Ported from python `analysis/resource_profiles.py` (`_puppet_key`, `augment_resource_profile_matching`,
`augment_resource_profile_changes`, `enrich_resource_profile_labels`) into the Rust finalize —
the first ResourceProfile language. hcl/dockerfile reuse the same mechanism when they route. The
`resource_profile_language` gate makes it inert for every other language. Residual noise (a
redundant `parameter_list` addition + child `string→variable` alongside the anchored changes) is a
known follow-up — the acceptance contract is satisfied. The MOVE→MODIFICATION relocation branch of
the python pass is not yet ported (the puppet playground doesn't exercise it).

## Empty-container labelling (elixir do_block, issue #62, 2026-07-08)

| If you see (X) | Expect (Y) | enforced by |
|---|---|---|
| A parser whose EMPTY block/body is a tree-sitter LEAF (no named children) | label it STRUCTURALLY (its node_type), never its source text — else a trivial-body → real-body edit flips the label (leaf-text → structural), which registers as a false leaf-delta and keeps a redundant parent MODIFICATION on top of the real body ADDITION | elixir `label_for` do_block case; crate pin `empty_and_filled_do_blocks_share_a_structural_label`; `test_scenarios_elixir::test_trivial_body_to_real_body` |

Fixed at the PARSER source (not the refinement), mirroring go/java whose blocks are never
text-labelled. Watch for the same pattern in other languages' body containers (perl, etc.).
