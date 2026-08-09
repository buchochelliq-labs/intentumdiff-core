# Code standard scenarios — "if you see X, expect Y" (every code language, same set)

The canonical scenarios every code language must satisfy, at **entity granularity**. Harness:
`tests/unit/test_standard_scenarios.py`. Node types vary per language; the expectation is the
*shape* (how many changes, of what kind, keyed on the entity name), not the exact node type.

| Scenario | If you see (X) | Expect (Y) |
|---|---|---|
| `add_fn_at_end` / `add_fn_in_middle` | a new function/class added | **one** `ADDITION` keyed on the entity name; no body/scaffold leakage |
| `delete_fn_at_end` / `delete_fn_in_middle` | a function/class removed | **one** `DELETION` keyed on the entity name |
| `modify_fn_body` | a statement/expression edited inside a body | **one** `MODIFICATION` on the changed node; **no** duplicate ADDITION of the new value |
| `rename_fn` | a function/class renamed (body unchanged) | **one** `REFACTORING` `RENAME_*`; **not** a MOVE + identifier modification |
| `rename_param` | parameters renamed (`a→x`, `b→y`) + body updated | one `REFACTORING` rename **each**, scoped; **no** param/operator `ADDITION`/`DELETION` leakage |
| `move_fn` | two functions reordered | a `MOVE` (or an intentional no-op) — but **not** classified as style-only/0-changes |
| `add_param` | a parameter added | one change for the signature (add/signature-change), no unrelated churn |
| `style_only` | whitespace/formatting only | `is_style_only=True`, zero changes |

## Gaps surfaced by the matrix (failing/xfail — real work, even in Python the flagship)

Discovered 2026-07-05 by running the standard matrix on Python (the certified batch language):

| Scenario | Current (wrong) | Expected | Status |
|---|---|---|---|
| `modify_fn_body` | ~~MODIFICATION **plus a duplicate ADDITION** of the same node~~ | one MODIFICATION `'Hi '`→`'Hello '` | **FIXED 2026-07-05** (issue #13): structural invariant `suppress_add_delete_drafts_covered_by_pairings` — a node already an endpoint of a paired change (MOD/REFACTORING/MOVE) is never also a bare ADDITION/DELETION. Rust test `paired_change_endpoints_suppress_duplicate_add_delete_drafts` |
| `rename_fn` | ~~`greet`→`greeting` gave a **MOVE** + identifier MODIFICATION~~ | one `REFACTORING` `RENAME_SYMBOL` (the redundant identifier mod is suppressed) | **FIXED 2026-07-05** (issue #10): `promote_same_id_named_renames_from_add_delete_drafts` now emits REFACTORING, not MOVE. Rust test `same_id_named_relabel_promotes_to_refactoring_rename_not_move`; Python `test_scenarios_python::test_rename_fn` |
| `rename_param` | `add(a,b)`→`add(x,y)` explodes into **6 changes** (param + binary_operator add/delete + identifier mods) | 2 scoped `REFACTORING` renames | xfail (issue #11) |
| `move_fn` | ~~reordering two functions → **style-only (0 changes)**~~ | one MOVE (not style-only) | **FIXED 2026-07-05** (issue #12): `suppress_low_signal_reorders_drafts` now keeps the longest-increasing-subsequence of sibling indices as insertion shifts (suppressed) and promotes order-inverted named entities to MOVE. Rust test `genuine_entity_reorder_promotes_to_move_insertion_shift_stays_suppressed` |

Note (candidate-signature level): unrenamed sibling functions that merely shifted lines still
surface as same-label line-shift `MOVE` drafts in the candidate signature — that positional-shift
accuracy question is issue #12 / backlog move-detection #2, distinct from the rename fix.

## Sweep findings 2026-07-05 (probe battery; tests in `test_scenarios_python.py`)

| Scenario | Current (wrong) | Expected | Status |
|---|---|---|---|
| `async_toggle` | ~~`def f` → `async def f` read **STYLE-ONLY**~~ (async token dropped by both CST serializers; `async_function_def` vocabulary was dead code) | a semantic change — never style-only | **FIXED** (issue #30): both converters emit `async_function_def`; Rust tests in rust-core-host + python-parser; acceptance `test_async_toggle` |
| `delete_and_add_unrelated` | deleted fn **vanishes**; 3 cross-matched integer MODIFICATIONs | DELETION `old_one` + ADDITION `new_one`, exactly 2 | xfail (issue #31, matcher position-pairing — same root family as #12) |
| `add_decorator` | wrapper ADDITION (no label) + `x→calc` pairing + **false MOVE** of untouched class | one change (the decorator) | xfail (issue #32, decorated_definition re-parenting) |
| `add_import_and_use` | return edit = DELETE+ADD of whole statement | import ADDITION + one return MODIFICATION | xfail (issue #33, similarity threshold; candidate fix = matched-parent same-type add+delete promotion) |

Clean in the sweep (no issues): `add_default_param`, `rename_method`, `rename_class` (the #10 fix
generalizes to methods/classes), `docstring_only`, `constant_value`, `fn_to_method` (1 change,
though arguably should read as a move-to-class — future refinement).

These are language-agnostic-shaped and likely recur across languages; they are prime targets for
the engine→Rust port (fix in Rust, assert in both layers). Line-oriented equivalents (prose
reorder; markdown section move) are tracked the same way in
[text-and-content.md](text-and-content.md).

## Cross-file scenarios (commit/index level — `analysis/cross_file.py`)

Multi-file refactors can't be seen by a single-file diff; they are detected over two
`SemanticIndex` snapshots by `detect_cross_file_changes`. Tests: `test_scenarios_crossfile.py`.

| If you see (X) | Expect (Y) | `ChangeType` | confidence |
|---|---|---|---|
| A symbol keeps its qualified name but its **file** changed (`foo` a.py → b.py) | one `MOVE_TO_MODULE` (`symbol_name`, `old_file`, `new_file`) | `MOVE_TO_MODULE` | 1.0 |
| Two+ symbols from one old file land in two+ new files (`a.py`{foo,bar} → b.py{foo}, c.py{bar}) | a `SPLIT_MODULE` (or the constituent `MOVE_TO_MODULE` moves) | `SPLIT_MODULE` | 0.9 |
| A symbol uniquely matched across files by `(file, node_type, parent_scope)` but with a new name | a `CROSS_FILE_RENAME` (conservative — only on an unambiguous match) | `CROSS_FILE_RENAME` | 0.8 |

Identity key note: a symbol's **qualified name** includes its scope (e.g. the module label), so a
move keeps the qualified name and only changes the file — that is exactly what `MOVE_TO_MODULE`
keys on. Don't vary the scope label across snapshots or the move won't match.

## Perl (fixed 2026-07-06, issue #23)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Body edit visibility | `print "Hello
"` → `print "Goodbye
"` inside a sub | The edit MUST surface (a string MODIFICATION or statement-level change). Zero changes / style-only is the #23 disease: the parser kind list once named a different grammar's node types and pruned every sub body. |
| Playground (use + body idioms) | adds `use strict/warnings`; `my $name = shift` → `my ($name) = @_`; string interpolation edit; `$a,$b` → `$x,$y` | 2 use ADDITIONs; greet's string MODIFICATION (`Hello, $name` → `${name}!`); the shift→@_ assignment change; NO MOVE of any `subroutine_declaration_statement` (anonymous-move noise). |

**If we see X then expect Y:** if a language's diff returns zero changes for a plain body edit → suspect parser kind-list drift (the crate's SEMANTIC_TYPES naming a different grammar's kinds than the one in Cargo.toml — delphi's camelCase `defProc` and perl's ts-parser-perl mismatch are both instances). Verify by walking the semantic tree of an added entity.

## Dart (fixed 2026-07-06, issue #21)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Body edit visibility | `return x + 1` → `return x + 2` | Exactly ONE MODIFICATION (`decimal_integer_literal` 1→2). Zero changes/style-only was the #21 disease: the kind list said `integer_literal` but the grammar emits `decimal_integer_literal` — the changed leaf was pruned and the trees hashed equal. |
| Playground (renames + concat→interpolation + added fn) | `add(a,b)`→`add(x,y)` block→arrow; `"Hello, "+name`→`"Hello, $name!"`; `multiply` added | a→x, b→y renames; ONE `multiply` function_signature ADDITION; NO add/delete of function_body/block/return_statement/additive_expression (string content labels make the interpolation edit pair as a modification instead of orphaned concat churn). |

**Routed to Rust finalize 2026-07-09 (#57 flip).** The body-reference rename (`a`→`x`, `b`→`y`) is single-letter, so the default path only classified it because `refactoring.py`'s `inferred_rename_pairs` corroborated it from the scoped param rename. The routed path had no equivalent → the identifier stayed a plain MODIFICATION (`_renames`==0). Fix: `promote_corroborated_variable_renames` in `lib.rs` (runs after the leaf-update promoters create the body identifier MODIFICATIONs) — anchors callables by (node_type, label) unique-in-tree, zips param NAME labels by position (cross-grammar via `is_parameter_list_type`, so `formal_parameter_list` works), infers `(old,new)` pairs, and relabels a matching body identifier MODIFICATION to `REFACTORING RENAME_VARIABLE`. Evidence-gated: a param **swap** `(a,b)→(b,a)` infers nothing (new name was an old param). **If we see X then expect Y:** a single-letter body identifier rename is promotable ONLY when an anchored callable's parameters rename the same pair at matching positions — never on label coincidence alone. Rust pin `corroborated_variable_rename_promotes_body_reference`; contract `test_dart_function_signature_anchoring_suppresses_body_scaffold_churn`.

**Kind-drift checklist (three confirmed instances):** delphi camelCase `defProc` vs lowercase profile; perl kind list from a different grammar entirely; dart `integer_literal` vs `decimal_integer_literal`. When auditing a parser, diff its SEMANTIC_TYPES against the grammar's `node-types.json` in the actual Cargo.lock-resolved crate, and ensure literal kinds carry CONTENT labels.

## ABAP (reformulated 2026-07-06, issue #20)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Changed form is anchored, not suppressed | greet gains USING param, DATA(lv_msg), writes lv_msg; add_numbers form added | ONE `entity_child_content_changed` MEANINGFUL group anchored on GREET; signature ADDITION mentioning LV_NAME; data_declaration LV_MSG ADDITION; write MODIFICATION 'Hello, World'→lv_msg; ONE compact form-level ADD_NUMBERS ADDITION (no descendant leakage); NO MOVE, NO GREET deletion. |

**If we see X then expect Y:** a matched named entity whose content changed anchors via the `entity_child_content_changed` group + fine-grained changes — NOT via a fabricated entity-level MOVE demoted to MODIFICATION. Tests asserting demote_stationary_*_move rules on stationary entities pin the retired move-fabrication pipeline; reformulate them to the group anchor.

## Elixir (fixed 2026-07-06, issue #22)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Playground (renames + interpolation + added def) | `add(a,b)`→`add(x,y)` do-block→`, do:` shorthand; `"Hello, "<>name`→`"Hello, #{name}!"`; `multiply` added | RENAME_VARIABLE a→x and b→y (same-id identifier del/add pairs promote — `refinement.same_id_identifier_rename`); string MODIFICATION for greet; ONE `call multiply` ADDITION; NO bare identifier x/y ADDITIONs; NO deletion mentioning Greeter. |

**If we see X then expect Y:** a DELETION and ADDITION of `identifier` nodes at the SAME position-path id are one renamed symbol → one REFACTORING(RENAME_VARIABLE), never add/delete leakage. Promotion is by exact id only — never sweep other changes by label equality (the #31/#32 swallow lesson).

## Puppet (fixed 2026-07-06, issue #24)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Attribute value change anchors on parameter identity | `message => 'Hello, World!'` → `message => $message` (+ class params, + file resource) | ONE `attribute message→message` MODIFICATION (same resource+attribute key, changed value hash); parameter/target/file-resource ADDITIONs; NO cross-pairing of the old value with the identical class-parameter default. |

## Dockerfile (routed to Rust finalize 2026-07-09, #57)

Reuses the resource-profile mechanism (puppet/hcl). Ported `dockerfile_key` to Rust so RUN/SHELL instructions key by a shell-command IDENTITY (`_docker_shell_identity` — first 2-3 tokens), not position.

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Insert a RUN; edit another; add ENV | insert `RUN apt-get update`; `RUN … compileall app`→`… src`; add `ENV APP_ENV=prod` (CMD, pip RUN unchanged) | ADDITION `run_instruction` (apt-get); ONE MODIFICATION `shell_fragment` `app`→`src`; ADDITION `env_instruction` (APP_ENV); NO add/delete of the unchanged pip RUN or CMD. **If we see X then expect Y:** an inserted RUN must NOT positionally cross-pair with an unrelated RUN (that swallowed the real compileall edit under routing) — RUNs pair by command identity. Rust pin `dockerfile_run_instructions_key_by_shell_command_identity`; contract `test_dockerfile_repeated_runs_do_not_swallow_cmd_or_env_changes`. |

## Assembly (`asm`) — statement profile (routed to Rust finalize 2026-07-09, #57; experimental)

The **statement-profile** family (asm/bash/delphi) parallels the resource-profile one. Ported the asm keying + `augment_statement_profile_matching` to Rust so statements pair by identity. asm instructions key by `("asm","instruction",section,mnemonic,first-operand,ordinal)` — the operand VALUE is deliberately excluded.

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Operand-value edit + inserted instruction | `mov ebx, 0`→`mov ebx, 42`; insert `add ecx, 8` | ONE MODIFICATION `instruction` (`mov ebx, 0`→`42`); ONE ADDITION `instruction` (`add ecx, 8`); NO DELETION+ADDITION of the mov. **If we see X then expect Y:** two instructions with the same mnemonic + first-operand register are the SAME instruction with a changed value → a MODIFICATION, never delete+add churn. Rust pin `asm_instructions_key_by_mnemonic_and_operand_identity`; contract `test_asm_statement_profile_preserves_operand_change_and_compact_additions`. |
| statement-profile scaffold suppression (mechanism; active for asm, needed by bash/delphi) | a MODIFIED **leaf** review container (command/assignment/instruction with a real label) whose body was edited | the leaf statement's sub-token churn (`expansion`/`word`/`string`; `exprBinary`) FOLDS into the single MODIFICATION — the general descendant-noise pass only roots on ADD/DELETE/MOVE, never a MODIFICATION. **If we see X then expect Y:** only LEAF statements fold; a SCOPE container (`function_definition`) must still surface its body's statement changes — a modified `f(){:}`→`f(){echo Hello}` stays DELETE+ADD of the `command`, not a folded word edit. `is_statement_scope_container` excludes function_definition. Rust pin `statement_container_modification_folds_descendant_token_churn`. **bash/delphi remain UNROUTED** pending their OTHER gap: bash — dissimilar sibling commands must UNPAIR into DELETE+ADD (routed core matcher pairs `:`↔`echo Hello` positionally); delphi — statement-level compaction (WriteLn matched as one unit, not its inner `literalString`). |

**If we see X then expect Y:** identical-label leaves in DIFFERENT enclosing entities never pair by line proximity ("reinserted nearby" requires the same scope); a same-key resource attribute with a changed value hash anchors as an attribute-level MODIFICATION even when the attribute LABEL is unchanged.

## JavaScript style compaction (fixed 2026-07-06, issue #26)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Deleted statements + arg-wrapping reflow + message edit | two console.log statements removed; call arguments re-wrapped across lines; 'Oh no! :(' → 'An error occurred:' | TWO clean statement DELETIONs (one per console.log); ONE MODIFICATION carrying the message edit; the `javascript.formatting.call_argument_wrapping_equivalence` IGNORED_STYLE group; NO fabricated MOVEs of identifier fragments across statements. |

**If we see X then expect Y (matcher invariants):** (1) a SAME-LABEL leaf pair must live inside matched-partner statements — post-matching statement-coherence prune, applied after all recovery so rename bootstrap is unaffected (an up-front gate starved `stage2` renames and was reverted); (2) a matched pair of call statements whose CALLEE labels differ (console.log ↔ console.error) is a dice artifact bootstrapped by a harvested leaf — dissolve the statement pair and every pair beneath it.

## SQL / TSQL / PLSQL (fixed 2026-07-06, issue #16 — 4th kind-drift instance)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Numeric literal edit | `SELECT 1;` → `SELECT 2;` | sql: exactly ONE MODIFICATION (`literal` 1→2); tsql/plsql: the edit VISIBLE (never style-only). The old kind list named a different grammar's vocabulary (select_statement/number vs sequel's select/term/literal), pruning every statement's interior. |
| Label casing | string literal `'hello'` | Literal/identifier content keeps EXACT text; only `keyword_*` kinds normalize to uppercase — `'a'` vs `'A'` in a literal is a real value change. |

## Return-expression rewrite (fixed 2026-07-06, issue #33 complete)

| Scenario | Input (old → new) | Expected |
|---|---|---|
| add_import_and_use | `return p` → `import os` + `return os.path.basename(p)` | exact_total = 2: ONE import ADDITION + ONE return_statement MODIFICATION. No p→os identifier noise, no separate call ADDITION. |

**If we see X then expect Y (decomposition invariants):** (1) positional leaf pairing inside a MODIFICATION is only valid when both sides have the SAME leaf count (shape-preserving edit) — different counts mean the value was rewritten, keep the statement-level pair; (2) when a non-leaf MODIFICATION's subtrees contain BOTH an inner DELETION and an inner ADDITION, those drafts double-report the rewrite and are suppressed — one-sided inner edits are honest partial changes and stay.

## Rust (`rust`) — entity-anchored matching (routed to Rust finalize 2026-07-09, #57/#60)

Unlocked by the anchors.py port (`augment_entity_matching` in `lib.rs`): entities key by
(node_type, enclosing-entity label path, label); same-key pairs recover by nearest line; content
zips by exact (type,label) **only within matched statement scopes** (id-prefix `entity_depth+2`).

| Scenario | Input (old → new) | Expected |
|---|---|---|
| match → if-let control-flow rewrite | `match port { Some(v) => format!(...), None => String::from(...) }` → `if let Some(v) = port { format!(...) } else { String::from(...) }` | ONE MODIFICATION (family `modification`) for the statement rewrite — the arm bodies (format!/call/tuple_struct_pattern) anchor via the matched statement pair, so the match/if containers pair as an in-place value rewrite. NEVER a bare DELETE match + ADD if that loses the relocated bodies. **If we see X then expect Y:** content zipped across UNMATCHED statements (a call leaving a deleted assignment into a new wrapper statement, the go anti-case) must NOT pair — the statement-scope gate blocks it, and the deleted statement stays a DELETION. Contract `test_competitive_synthetic_fixture_diff_contract[rust-match-to-if-let]`; pin `entity_anchoring_recovers_pairs_and_gates_zips_by_statement_scope`. |

**Guards that keep anchoring honest (all contract-pinned):** name nodes never align positionally
(no invented cross-name renames); param zips need equal counts (count mismatch = signature change);
one RENAME_VARIABLE per (old,new) pair (occurrences are corroboration, not events); zips respect
statement scopes.

## JavaScript family (`javascript`/`typescript`/`tsx`) — routed to Rust finalize 2026-07-15, #57

Three ports closed the js-family gap; each carries an anti-fabrication lesson learned from
oracle-rejected drafts (see BACKLOG "#57 endgame frontier" for the rejected approaches).

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Moved function with internal edit | nested `calc_hash` (md5, readFileSync(path)) hoisted to top level with `createHash('sha256')` + `readFileSync(path, {flag:'r'})` | exactly ONE MOVE (calc_hash) + MODIFICATION md5→sha256 + the `'r'` flag surfacing; NO ADDITION/DELETION/REORDER. The edit script prunes unmatched leaves under moved subtrees, so internal edits of a moved function need the **moved-pair literal update recovery**: within each MOVE pair, unmatched literal-like leaves pair by the #37 score (0.6 + position + first-char + parent-label, cap 0.95) and REQUIRE the same parent node_type — that guard is what excludes `'r'` (inside a pair/object) from stealing md5 (inside arguments). Contract `test_stage4_javascript_moved_code_*`. **Three anti-fabrication guards, all earned from real failures:** (1) equal-label leaves consume each other SILENTLY before scoring — skipping them instead let the greedy residue cross-pair (go's relocated `add(3, 4)` fabricated 3→4 + 4→3 int_literal updates, which then fed the inverse-coverage arm and swallowed the deleted assignment); (2) recovery is GLOBALLY unique per leaf across nested MOVE pairs (a container and its parent can both move; without this md5 paired with BOTH sha256 and 'r'); (3) the nearest-labeled-ancestor context bonus (+0.15) — same declarator name (`hash`) outweighs raw line proximity, else `'r'` (under `bytes`, one line closer) beats `'sha256'` (under `hash`). |
| Function-valued declaration | `function circleArea(r){…}` → `const circleArea = (r) => …` | The lexical/variable declaration whose declarator value is a function IS a function_declaration named by the declarator (derived kind in `anchor_entity_key` + `anchor_is_function`) — pairs cross-type as ONE entity, never DELETE function + ADD const. |
| Rename with usage churn | `list_files(dir)` → `list_files(directory)`; body uses `dir` in `fs.readdirSync(dir)` | REFACTORING rename reported ONCE per (old,new) pair; the same-label `call_expression` DELETE+ADD around the renamed argument suppresses ONLY because the subtree label delta ({dir}→{directory}) is fully covered by the reported rename — the **rename-coverage guard** (tighter than python's `_suppress_same_label_add_delete_pairs`). **If we see X then expect Y:** a same-label container pair whose delta is NOT rename-covered (go `return err` → `return fmt.Errorf(…)`, labels being type-fallbacks) is the ONLY representation of a real edit and MUST survive — pin `dying_no_delta_modification_cannot_swallow_its_add_delete_pair`. |
| Leaf pairing inside DELETE+ADD ancestors | a paired leaf MODIFICATION whose old endpoint sits inside a bare DELETION subtree and new endpoint inside a bare ADDITION subtree | the DELETE+ADD container pair is the SAME edit at container granularity → BOTH suppressed (inverse arm of `suppress_add_delete_drafts_covered_by_pairings`); one-sided containers stay (honest partial edits). |
| Style-only reformat (stage 3) | call/argument re-wrapping + one real string edit | exactly the real changes + an IGNORED_STYLE group `javascript.formatting.call_argument_wrapping_equivalence` with provenance "suppression" (relabelled residue, never an equivalence proof — #51). |

## Python routing tiers (#57 flip 2026-07-15)

Python's default path is served by THREE Rust tiers in order; know which one you're testing:

| Tier | Trigger | Contract |
|---|---|---|
| Certified batch (`try_rust_core_batch_diff`) | default config, no guardrail candidates, no enrichers | `semantic_contract: rust_finalized_v1`; sources→final entirely in Rust; does NOT evaluate guardrails (that's WHY it's skipped when `guardrails_may_apply`) |
| 9-fin finalize routing (`try_rust_finalize_review`) | batch declined; language in `RUST_FINALIZE_LANGUAGES`, no enrichers/analyzers | `semantic_contract: rust_finalize_review_v1`; trees→final in Rust; applies `apply_guardrails_to_diff` on the routed diff |
| Python stages (transitional) | both Rust tiers declined (diagnostics, enrichers, plugins) | slated for deletion with `analysis/*` |

**If we see X then expect Y:** a test asserting "the batch path was skipped" must accept ANY
guardrail-evaluating tier (full pipeline phases OR `rust_core.stage == per_stage_finalize_routing`
with the `rust_finalize_review` phase) — pinning "no rust_core metadata at all" conflates
"batch skipped" with "python pipeline ran". The stage-11 hybrid (python stages 1–11 + Rust 12+)
no longer serves the default path; it ran python `promote_moves` and was the last default-path
`analysis/*` consumer for python.

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Comment-only insertion (style-only hunks) | `x = 1

y = 2` → same with `# first`/`# second` comment lines added | style-only diff, zero changes, `generic.style_only_shortcut.source_equivalence` group per hunk. **The trap:** python's full-parse trees KEEP comment nodes, so every later sibling's position-path id shifts one slot — old `y = 2` lands on new `x = 1`'s id. `promote_tree_leaf_value_updates_drafts` (same-id tree-scan promoter) fabricated `integer 2→1`; the issue-#31 same-entity guard is VACUOUS at module level. Fix: the nearest LABELED ancestor must agree on both sides ('y' vs 'x' rejects the shift; a real `x = 1→x = 2` update keeps its assignment label). Any same-id promoter needs a sibling-shift guard — position paths are only stable ids when no sibling was inserted above. |

## Diagnostics-tier closure (#57 payoff stage 4a, 2026-07-15)

**The lesson:** `DiffConfig(diagnostics=True)` suites were a HIDDEN pocket of python-only
coverage — they forced the python pipeline, so per-language routing flips never gated those
scenarios. Closing the tier surfaced real routed gaps that shipped broken on the default path:

| Scenario | Input (old → new) | Expected |
|---|---|---|
| Exported function swap (ts) | two `export function`s swap order | ≥1 MOVE + `refinement.entity_reorder_to_moved_code` MOVED_CODE group (LIS: a swap is ONE move). The reorder-promotion entity gate must accept **export wrappers** (`export_statement` with an entity child — python `_is_exported_entity_wrapper`) and **doc components** (`jsx_component`/`section`/`markdown_section` — python `_LEAF_ENTITY_TYPES`); without them the swap yielded ZERO changes (the exact #66 powershell failure mode, wider vocabulary). |
| Function rename (ts/ps) | `formatOrder` → `formatOrderLabel` everywhere | ONE REFACTORING pair (routed presents rename-as-REFACTORING — a richer contract than python's rename-as-MODIFICATION; scenarios updated). A MOVE whose endpoints are BOTH inside the refactoring pair's subtrees is stationary — the container renamed around it (`suppress_child_moves_under_refactoring_pair_drafts`; without it `formal_parameters`/`statement_block` leaked as MOVEs). |
| mdx component prop edit | `<Step name="Verify" status="pending"/>` → `status="ready"` | MODIFICATION + `refinement.entity_child_content_changed` group anchored to **'Step Verify'** (the component), not the raw attribute string — `jsx_component` is an entity container. |
| Diagnostics trace (#54 MVP) | any diff with `diagnostics=True` | stages `{parser, parse, finalize}`; finalize carries `rust_finalize_review` + per-pass `refine:*` events (thread-local collector in `finalize_debug_probe`, `collect_trace` config flag). Trace-shape pins are reformulated behavior-level: the diff proves WHAT, the trace proves the machinery RAN. |
