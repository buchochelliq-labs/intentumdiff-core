# Reviewed baseline after the first correctness fixes

CI now protects **578 passing cases across all 74 language IDs** in the unchanged
760-case inventory. The [baseline JSON](2026-11-08-post-fix-language-baseline.json)
is an exact copy of the retained summary from the first combined repair run.
It preserves all case identities, expected verdicts, statuses and diagnostic
fields. The [original assessment](2026-11-08-language-assessment.json) remains
unchanged as the historical 559-pass baseline.

The combined fixes add 19 passes with no regression among the original passes.
The other outcomes remain explicit: 104 failed, 3 invalid results on incomplete
input, 43 bounded incomplete observations and 32 explicit fallbacks. The baseline
protects observed successes; it does not certify full language correctness or
bless the remaining failures. Later Python literal and long-literal repairs are
outside this baseline.

## Provenance

| Evidence | Identity |
| --- | --- |
| Retained run timestamp | `2026-11-08T18:17:36.055927+00:00` |
| This baseline / retained summary SHA-256 | `406683bac6782aaf09831e4c656a018ced9b08f7fe5c3556c3e53150b3e122ef` |
| Original baseline SHA-256 | `70461ae8bc1dc55ad9ca4a203db5c1e75a0ee89ecdcacb859f325d447119dda6` |
| Native C ABI SHA-256 | `dff49b1f0614c8da78ef22569d85c1f50250050c60a91213fa912757bb9b8c5c` |
| Native source: local combined verification commit | `96d5ddc75f2f4e37d9477ef1a4bc57dbc464e9af` |
| Native source: repository tree | `cfdd3d914a762ab51b90b9d9e127f73c8392c2d1` |
| Native source: `crates/rust-core-host` tree | `af5a59f1c3901915e7ae1afbf13dec3e6bdb0c66` |
| Collector source: local verification commit | `7eeeaf18cbb414a5ad5e3ddd571170263512ed56` |
| Collector source: `scripts/assess_languages.py` Git blob | `f223fe0db778a9163d18167eb8b1a53549333ac0` |
| Fixed inventory SHA-256 | `37f27f7b6ea8c21b6f7af3f07a189d0c76d73ab8ea316f96eea20ef5559db4d7` |
| Staged parser manifest SHA-256 | `2be61b459c125dbaa28c55bffaff7a3ac56a558c24f933650ffd19094e4332b9` |

The local combined verification includes the reviewed repairs in
[core#281](https://github.com/buchochelliq-labs/intentumdiff-core/pull/281),
[core#282](https://github.com/buchochelliq-labs/intentumdiff-core/pull/282),
[core#283](https://github.com/buchochelliq-labs/intentumdiff-core/pull/283) and
[core#284](https://github.com/buchochelliq-labs/intentumdiff-core/pull/284), on the
[foundation](https://github.com/buchochelliq-labs/intentumdiff-core/pull/269).
The collector is the [regression guard](https://github.com/buchochelliq-labs/intentumdiff-core/pull/286).
These identities describe the tested local composition, not a release tag.

The summary retains the complete parser manifest and all four fixture-file
SHA-256 hashes. It already omits full reviews and parse trees, which remain in
the retained raw run artifacts. Historical absolute paths and timing fields are
run provenance, not portable download links or baseline acceptance criteria.
The JSON's `baseline_gate` records that run's comparison against the original
559-pass baseline; the refreshed CI gate derives its 578 protected passes from
the per-case statuses in this new file.

Use the command in [the assessment guide](../LANGUAGE_ASSESSMENT.md#gate-candidate-changes-against-the-reviewed-baseline)
with this file as `--baseline`. Every case must retain its source hashes and
oracle. No fixture, verdict, status or validation rule changed during refresh.
