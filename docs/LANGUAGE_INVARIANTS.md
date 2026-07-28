# Language invariants

Invariance rules let the engine hide, group, or downgrade changes that do not affect the
meaning a reviewer cares about — **only when it can explain why**, and always preserving the
raw evidence in metadata/`change_groups`.

## Taxonomy

`syntactic_trivia` · `grammar_equivalence` · `canonical_value_equivalence` ·
`unordered_collection_equivalence` · `contextual_equivalence` · `refactoring_equivalence`

## Rule shape (the data-driven catalog)

Rules live in the shipped `rules.yaml` catalog (validated against its schema) with: a stable
`id` (e.g. `python.string.escape_equivalence`), `languages`, the pipeline `stage`, a
`guarantee` level (spec/parser/project/heuristic), firing `guards`, an optional
`canonical_value`, preserved `evidence`, and a `risk` grade:

- **green** — safe from syntax/value semantics alone (hidden by default)
- **amber** — safe only with explicit guards or project context
- **red** — never hidden by default; grouped or annotated instead

Evaluation happens in the engine (`invariance_groups`); the allowlisted evaluator set rejects
arbitrary code in rule definitions.
