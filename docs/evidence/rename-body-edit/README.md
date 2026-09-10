# Rename with body edits: development evidence

Tracks [core#44](https://github.com/buchochelliq-labs/intentumdiff-core/issues/44),
the rename/body-edit portion of [Python#45](https://github.com/buchochelliq-labs/intentumdiff-python/issues/45).
This is development-build evidence on Linux x86_64, not release certification.

## Expected behavior

Compare [old.py](old.py) and [new.py](new.py): the original function continues as
`compute_order_total`, while the cap changes from `50` to `75`. Both facts must be visible.
The newly added `_subtotal` must never be mistaken for the renamed original function.
Helper extraction classification remains outstanding; this change keeps the helper and call
visible as additions and does not close the complete Python issue.

![Actual CLI output, recorded through Rich and rendered to PNG](cli.png)

[Plain CLI output](cli.txt). The capture invokes the source checkout's real `file` command,
records its Rich console, then renders the SVG to PNG. The banner's existing `v0.0.1` value
comes from that checkout; it is not a new version or a released-wheel claim.

## Diagnosis and boundary audit

Default Python-language `SemanticDiffer` returns Rust's native batch result. Diagnostics
uses the real Wasm parser, Rust tree finalisation, and remaining Python postprocessing.
Removing the named-label gate alone did not fix the minimal case: the measured function Dice
score was zero, with only module roots matched. Renamed scopes blocked the descendant seeds
needed by bottom-up matching. The fix seeds conservative callable continuity before those
scope gates, without removing the unchanged-body safety guard on whole-subtree collapse.

Python still evaluates invariance rules, enriches labels and makes some style-equivalence
decisions. This is active processing, tracked in [Python#54](https://github.com/buchochelliq-labs/intentumdiff-python/issues/54)
and [core#39](https://github.com/buchochelliq-labs/intentumdiff-core/issues/39).
Direct Python/Rust integer comparisons agree with independently stated expectations:
`1→0x1` and `1_000→1000` are equivalent; `50→75` and
`9007199254740992→9007199254740993` remain changes. This is not a claim of full evaluator parity.

## Reproduction

Build the core, stage the parser components in the Python checkout, then use that freshly
built cdylib through `_CtypesBackend`. Confirm the loaded library path before running:

```sh
cargo test --manifest-path crates/rust-core-host/Cargo.toml --no-default-features --lib
cargo rustc --manifest-path crates/rust-core-host/Cargo.toml --no-default-features --lib --crate-type cdylib
python -m pytest tests/unit/test_rename_body_edit.py tests/unit/test_scenarios_python.py tests/unit/test_scenarios_javascript.py -q
python -m intentumdiff file old.py new.py
```

Core baseline: `80003118bff83eba645ad259bc9a44d36f63a5ab`.
Python baseline: `06f0dcc1830f648f413f2a0ed45e802f436e1e53`.
Parser sources built with Rust 1.98.1 for `wasm32-wasip2`:

- Python: `4d825be9e26f96871539b990d1171b1815b8538f`.
- JavaScript/TypeScript: `c7ae6476889dd0db4a88907735ed06c7f155d07b`.
- Both pin SDK `v0.0.2-beta.1` (`eab92af5`).

Core correctness iterations use the unoptimised profile with debug information disabled.
Release builds, all supported platforms, and the complete parser estate remain release gates.

## Independent review

An independent agent found two blockers in the initial candidate: matching across different
outer classes with the same inner class name, and losing reordered dependent statements under
a renamed function. Both examples were added as regressions. The candidate now checks full
enclosing scope and preserves executable statement reorder evidence as meaningful changes.
The rename group owns the declaration identity, not its entire body; final Rust routes create
their own meaningful groups instead of relying on Python to fill them in.

The reviewer also found a pre-existing ambiguous-source selection defect, tracked separately
in [core#45](https://github.com/buchochelliq-labs/intentumdiff-core/issues/45).

Final independent review confirmed the scoped fix: literal edits and executable reorders
remain visible, cross-scope false matches are rejected, groups own separate final indices,
and contradictory formatting/moved-code groups are absent for executable reorders.
Suppressed positional shifts during a callable rename are not proof of formatting equivalence.
Existing decorator-order loss remains a separate limitation, as does helper extraction.

## Verified results

- Core library suite: **263 passed, 25 ignored** with `--no-default-features`.
  The ignored Tier-C checks require their component configuration; they are not counted as passes.
- Public Python/native and real Wasm parser checks: **50 passed, 2 expected failures**.
  The existing JavaScript expected failures concern added-parameter and import/use noise.
- Baseline comparison: both minimal and full rename/body-edit regressions failed on the
  original RC, while two conservative controls passed.
- Independent focused Rust review: **5 passed, 1 Wasm-dependent check skipped**;
  direct probes additionally checked the freshly built C ABI library.

Tests used a fresh temporary directory because the analytics fixture's process-ID database
name can collide with a stale file across runs. All evidence above uses the final source
candidate and the explicit rebuilt cdylib, not a stale shared build artifact.
