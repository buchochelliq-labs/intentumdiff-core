# Change-group index-space contract

## The problem it prevents

`SemanticDiff.change_groups[*].raw_change_indices` are consumed as indices **into the final
`SemanticDiff.changes` array** by the VS Code extension (`reviewModel.ts`, `intentCodeLens.ts`)
and release notes (`releaseNotes.ts`). But groups are assembled at different pipeline stages,
each against whatever `changes` list existed at that moment, and later stages (e.g.
`sort_changes_by_position`, array-index suppression, file-lifecycle) **re-order and filter**
`changes`. A group's positional indices then point at the wrong (or a non-existent) change.

Real symptom that motivated this contract: a `NOISE_SUPPRESSED` group carrying stale
`presentation_input` indices `[0..86]` collided with a genuine `export_statement` addition at
final `changes[3]`, so a brand-new function rendered as **"Noise · below the meaningfulness
threshold — suppressed."** Meanwhile the real `MEANINGFUL_CHANGE` group owned *no* indices.

## The three index spaces

Groups tag `metadata.index_space`:

| Space | Meaning |
|---|---|
| `final_changes` | Indices already address the final `changes` array. |
| `presentation_input` | Indices address a *pre-refinement* change set that no longer exists in the final array. Cannot be trusted as-is. |
| `mixed` | Aggregated from multiple source groups across spaces. |
| *(other, e.g. `style_only_shortcut`)* | Specialised evidence that keys off source spans, **not** change indices — `raw_change_indices` is empty and must be left untouched. |

## The single reconciliation: `_reindex_groups_to_final_changes`

Located in `src/intentumdiff/analysis/presentation.py`, applied as the **last** transform over
`diff.change_groups` in `differ.py::_complete_final_diff` (after sorting and file-lifecycle,
before telemetry/caching).

It is a deliberate **boundary reconciliation**, not a patch over individual producers: rather
than make every mutating stage remap every group (fragile, many call sites), re-establish the
invariant once at the end using the one key stable across re-sorts/filters — **node identity**.

Algorithm per group:
- If the group carries node ids (`new_node_ids`/`old_node_ids`): index `i` is owned iff
  `changes[i]`'s `new_node.id`/`old_node.id` is in that set. This is authoritative regardless
  of the original space.
- Else if `index_space ∈ {final_changes, presentation_input, mixed}` with no node ids:
  `final_changes` → keep in-range indices; `presentation_input`/`mixed` → **empty** (they
  addressed a set that no longer exists, so they own nothing final).
- Else (specialised evidence space, or no `index_space`): **leave the group untouched** —
  retagging would erase its provenance metadata.

Labels and `metadata` (including `suppressed_count`) are preserved, so evidence/counts are
unchanged; only the mis-addressing goes away.

## The producer contract (fix at the source, not downstream)

Every `ChangeGroup` producer must satisfy one of:
1. Carry the node ids of what it groups (→ the reindex derives correct final indices), **or**
2. Be honestly empty when it owns no final change.

**Anti-pattern (do not do this):** `normalize_generic_text_for_review` originally created its
`NOISE_SUPPRESSED` group with `raw_change_indices=range(len(input_token_changes))` — indices
into a list it *discards and replaces* with line spans. Those indices were invalid the moment
they were created and collided with the replacement. The correct fix was at the source: emit
`raw_change_indices=[]` (it owns no final change; `suppressed_count` still drives the
"(N hidden)" label) — **not** to tag it so the reindex would erase the phantom indices later.
That downstream-erase approach is a "stick plaster on a plaster." Audit new producers the same
way: if a group references a discarded/pre-refinement change set, it owns nothing final.

## Consumer contract (extension + release notes)

- Range-check every dereference: skip `i < 0 || i >= changes.length`.
- A change owned by **no** group is a real ungrouped change — classify it via
  `kindForChange(change)` (→ `MEANINGFUL_CHANGE` for a new function) rather than dropping it.
  Helpers: `coveredChangeIndices(groups, changeCount)` unions in-range group indices;
  `intentCodeLens.ts`, `reviewModel.ts`, and `releaseNotes.ts` all fall back per-change.
- The engine *could* attach a `MEANINGFUL_CHANGE` group to ungrouped meaningful changes to
  make the extension fallback redundant — noted in `docs/BACKLOG.md`, not yet done.

## Tests

`tests/unit/test_reindex_groups.py` covers: node-identity remap, noise group emptied,
out-of-range dropped, deletion `old_node` match, and the generic-text group emitting `[]` at
the source. Extension side: `intentCodeLens.test.ts` / `reviewModel.test.ts` cover the
ungrouped-meaningful promotion and out-of-range handling.
