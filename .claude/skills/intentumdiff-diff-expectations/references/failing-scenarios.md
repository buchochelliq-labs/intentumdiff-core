# Failing-scenario catalogue (current migration targets)

The 10 scenarios that are still red (RC blockers), grouped by profile family. Each is the
acceptance spec a Rust port must satisfy. Inputs are playground fixtures
(`tests/fixtures/semanticdiff_examples.json`, via `_playground_diff(lang)` / `_diff(lang, name)`)
unless a literal old→new is given. "MUST" = an assertion that must hold; "MUST NOT" = a no-leakage
assertion.

The playground examples share a shape: `old` defines routines (e.g. `greet` with params `a,b` and
a greeting expression); `new` renames params `a→x`, `b→y`, changes the greeting, and **adds** a
function (`multiply` / `cube` / `Multiply-Numbers` / `ADD_NUMBERS`). The expected review is: param
renames as renames, the added function as one entity addition, body/scaffold churn suppressed, no
false moves.

---

## Statement profiles (asm / bash / delphi) — `analysis/statement_profiles.py`

### bash — `test_bash_statement_profile_suppresses_expansion_churn_and_labels_commands`
- **input:** `_playground_diff("bash")`.
- **MUST:** exactly 1 `MODIFICATION` on `variable_assignment` `NAME=$1` → `NAME=${1:-World}`.
- **MUST:** `ADDITION` labels include `set -euo pipefail`, `greet`, and `greet "$NAME"` (commands
  labelled by their full command text, keyed as commands).
- **MUST NOT:** an added label `-euo` (don't split a command's words) or `command` (don't surface
  the bare node type as a label).
- **MUST NOT:** any `ADDITION`/`DELETION` whose node_type ∈ {`expansion`, `simple_expansion`,
  `word`, `string`} (expansion/word churn suppressed as scaffold).
- **why:** bash review should read as commands + assignments, not tokenized shell-word churn.
- **profile roles:** `command`/`declaration_command`/`function_definition`/`pipeline`/
  `variable_assignment` = keyed+review; `command_name`/`compound_statement`/`word`/`expansion`/
  `string`/… = scaffold (suppressed).

### delphi — `test_delphi_statement_profile_compacts_changed_greet_expression`
- **input:** `_playground_diff("delphi")`.
- **MUST:** exactly 1 `MODIFICATION` on the greet statement `WriteLn('Hello, ' + Name)` →
  `WriteLn(Format('Hello, %s!', [Name]))`.
- **MUST NOT:** a `DELETION` of node_type `exprBinary`; an `ADDITION` of node_type `statement` with
  label `statement`; any add/delete of `moduleName` `Demo` (module header is scaffold).
- **MUST:** `ADDITION` labels include `WriteLn(Multiply(2, 3))` (the new call surfaces as a
  statement labelled by its text).
- **why:** a changed statement is one modification, not a spray of expression-node add/deletes.

### delphi (scoping) — `test_delphi_statement_matching_is_scoped_to_owning_routine`
- **input (literal):** two procedures `Alpha`/`Beta`, each `WriteLn('Alpha')`→`WriteLn('Alpha
  changed')` and `WriteLn('Beta')`→`WriteLn('Beta changed')`.
- **MUST:** structured (no fallback/parse errors); no moves/reorders; a `MODIFICATION`
  `Alpha`→`Alpha changed` (and Beta likewise), each matched **within its owning routine** (Alpha's
  statement doesn't match Beta's).
- **why:** statement matching must be scoped per routine, or identical-shaped statements in
  sibling routines cross-match.

---

## Entity / function anchoring — `analysis/language_profiles.py`

### abap — `test_abap_changed_form_is_not_suppressed_as_stable_noise` (`_abap_shallow_form_diff()`)
- **MUST:** 1 `MODIFICATION` on `form` `GREET`→`GREET`, mentioning new `LV_NAME` and `LV_MSG`.
- **MUST:** 1 `ADDITION` `data_declaration` `LV_MSG`; 1 `MODIFICATION` `write_statement`
  `'Hello, World'`→`lv_msg`; 1 `ADDITION` `form` `ADD_NUMBERS` with **no descendant leakage**
  (no ADDITION whose id starts with the new form's id + ".").
- **MUST NOT:** a `DELETION` of `form` `GREET`; a `refinement.demote_stationary_function_move`
  group. **MUST:** a `refinement.demote_stationary_entity_move` OR
  `refinement.demote_same_scope_entity_move` group.
- **why:** a form whose body changed is a modification; a new form is one entity addition; the
  stationary GREET must not be demoted as a *function* move nor deleted.

### dart — `test_dart_function_signature_anchoring_suppresses_body_scaffold_churn`
- **MUST:** `a→x` and `b→y` renames (1 each); no `MOVE`.
- **MUST NOT:** any add/delete of node_type ∈ {`function_body`, `block`, `return_statement`,
  `additive_expression`}.
- **MUST:** exactly 1 `ADDITION` `function_signature` `multiply`.
- **why:** anchor on the function signature; suppress body scaffold; renames stay renames.

### elixir — `test_elixir_definition_calls_anchor_without_parameter_leakage`
- **MUST NOT:** a `DELETION` mentioning `Greeter`; an `ADDITION` `identifier` `x` or `y`.
- **MUST:** exactly 1 `ADDITION` `call` `multiply`.
- **why:** anchor on the definition/call; don't leak renamed parameters as identifier additions;
  don't delete the surrounding module.

### perl — `test_perl_named_subroutines_are_not_reported_as_anonymous_moves`
- **MUST NOT:** any `MOVE` of node_type `subroutine_declaration_statement`.
- **MUST:** at least one change mentioning `greet` (the real edit surfaces).
- **why:** named subs shifting position are not anonymous moves.

---

## Resource profile — `analysis/resource_profiles.py`

### puppet — `test_puppet_playground_uses_resource_titles_and_parameter_identities`
- **MUST:** structured (no fallback/parse errors); a `MODIFICATION` on `attribute` `message`→
  `message` (attribute matched by name).
- **MUST:** `ADDITION` labels include `message`, `target`, and `file /tmp/greeting.txt` (a new
  resource keyed by type+title).
- **MUST NOT:** a `MODIFICATION` `hello`→`/tmp/greeting.txt` (don't cross-match unrelated attribute
  values by position).
- **why:** resources are identified by type+title and attributes by name, not position.

---

## Matcher / presentation (JavaScript) — `analysis/refinement.py` / `presentation.py`

### javascript moved-code — `test_stage4_javascript_moved_code_has_no_add_delete_leakage` (`_diff("javascript", "Moved Code")`)
- **MUST:** exactly 1 `MOVE` (of `calc_hash`); a `MODIFICATION` `md5`→`sha256`; a change
  `readFileSync`→`flag`.
- **MUST NOT:** any `ADDITION`, `DELETION`, or `REORDER` (a move must not leak add/delete/reorder).
- **why:** genuine moved code is one MOVE plus the real edits, nothing else.

### javascript style — `test_stage3_javascript_style_is_compacted` (`_diff("javascript", "Style Changes")`)
- **MUST:** exactly 2 `DELETION`s (`console.log(foo)`, `console.log(bar)`); exactly 1
  `MODIFICATION` `'Oh no'`→`'An error occurred'`.
- **MUST:** an `IGNORED_STYLE` group with rule `javascript.formatting.call_argument_wrapping_equivalence`,
  a non-empty `metadata.reason`, and non-empty `old_node_ids`/`new_node_ids`.
- **MUST NOT:** a generic `presentation.ignored_style.javascript` group (use the specific language
  rule, not the generic fallback).
- **why:** argument-wrapping reflow is ignored-style with an explainable, node-id-keyed reason;
  real deletions/modifications still surface.

---

## Port checklist (per scenario)
For each row: (1) confirm this expectation is what we want; (2) implement in the Rust profile-family
stage until it holds; (3) add a Rust `#[cfg(test)]` asserting it; (4) route the language through
Rust (allowlist) so the Python test above passes through Rust, incl. under
`INTENTUMDIFF_ENFORCE_RUST_ONLY_ENGINE=1`; (5) retire the Python profile branch.
