# Language correctness assessment

The [2026-11-08 diagnostic scorecard](evidence/2026-11-08-language-assessment.md)
covers all 74 registered language IDs using 760 independently source-judged
probes. It records failures and remaining coverage gaps. It does not certify
language correctness or replace the [external corpus admission
requirements](EXTERNAL_BENCHMARKS.md).

## Reproduce the observations

Use a candidate installation of `intentumdiff-python` containing the native Rust
library being assessed. Stage the registry-verified components as described in
the external benchmark guide, including the canonical candidate C/C++ component.
The assessment records the installed native library's SHA-256 and the complete
parser manifest; compare these identities before comparing two runs.

```bash
python -m pytest tests/tooling -q
python scripts/assess_languages.py \
  --fixtures tests/fixtures/language_assessment \
  --parsers build/benchmark-inputs \
  --report build/language-assessment/assessment.json \
  --jobs 4 --timeout 120
```

The command is a diagnostic collector. Its process exit status is not a semantic
acceptance gate: inspect per-case statuses, worker errors and `not_executed` as
well as the totals. `--languages python rust` selects a smaller reproduction.
Raw reviews and parse trees are retained beside the report in `raw/<id>.json`.
Interrupted workers preserve completed observations; an unobserved tail is
`not_executed`, never a passing or completed case.

The tool calls the production Rust C ABI with an explicit language hint and
strict component selection. Python review uses the engine's native tree-sitter
path after component selection. Generic review uses native text handling, with
its Wasm component used for separate tree inspection. Other IDs use the selected
Wasm component through the Rust host. Python here only defines probes, invokes
the engine and records results.

## Read the evidence

The four fixture families retain exact before/after strings, expected verdicts,
source rationale, limitations and provenance. Retained upstream files have
immutable revisions, licenses and byte hashes. An external-derived mutation or
identity pair is not upstream edit gold. OCaml and R source-specific admission
is explicitly blocked; other derived sources remain admission candidates.

The assessment checks headline verdicts, routing, repeatability, source bounds,
UTF-8 boundaries, node IDs and final change-group references. Timing and fuel
measurements are excluded from repeatability comparisons. Exact source-span
accuracy and explanation/correspondence precision or recall are not automated
by these checks. The independent review adds three concrete explanation or
evidence defects that passed headline verdict checks.

An incomplete-input observation means the engine returned within the bounded
run, either with a result or an explicit fallback. It is not a correctness pass.
Parent-child span containment is retained as an observation: a node's name span
can legitimately exclude its descendants.

## Repair order and completion criteria

1. Repair shared core losses and invalid evidence: YAML suppression, normalized
   label/hash projection, embedded style/string handling and evidence offsets.
2. Repair parser payload and modifier omissions, then false positives from
   comments, spacing and equivalent syntax. Reuse existing tickets where the
   assessment reproduces an already tracked defect.
3. For each repair, add a focused regression, adjacent preservation controls and
   independently reviewed upstream cases. Replay affected IDs and the shared
   external gate before claiming the defect resolved.
4. Acquire external sources for the 35 IDs with no retained upstream cases,
   resolve the two blocked source admissions and expand construct coverage,
   exact changed-entity/span expectations, dialects and real repository edits.
   Complete platform, sustained resource and fuzz acceptance separately.

Every confirmed diagnostic finding has an issue link in the scorecard. Passing
the current probes closes only those probes; full-language assessment requires
the broader evidence and acceptance lanes recorded above.
