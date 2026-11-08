# Reviewed 584-pass baseline from the merged release candidate

CI now protects **584 passing cases across all 74 language IDs** in the unchanged
760-case inventory. The [baseline JSON](2026-11-08-merged-rc-language-baseline.json)
is an exact byte copy of the successful merged-RC CI summary. It preserves all
case identities, expected verdicts, statuses and diagnostic fields.

All 578 passes from the [first repair baseline](2026-11-08-post-fix-language-baseline.md)
remain passes. Six additional cases now pass: Python bytes/text literal-kind
handling and the long-literal suffix cases for JavaScript, TypeScript, JSON, SQL
and YAML. The remaining outcomes stay explicit: 98 failed cases, 3 invalid
results, 43 bounded incomplete observations and 32 explicit fallbacks. Promoting
observed successes does not certify full language correctness or bless failures.

The historical [559-pass assessment](2026-11-08-language-assessment.json) and
[578-pass baseline](2026-11-08-post-fix-language-baseline.json) remain unchanged.
No fixture, oracle, parser inventory or validation rule changes in this refresh.
The new protection derives solely from the previously executed case statuses.

## Provenance

| Evidence | Identity |
| --- | --- |
| Source branch | `release/v0.0.3-rc` |
| Exact merged source commit | `9e5021ae362fc44f8f28f2f213ca4dabf5daae1b` |
| Source repository tree | `65452eb5b9cfbe124000de830e8169afbe20e96e` |
| Source `crates/rust-core-host` tree | `8eaa4866489bbc5ac1c41a703c41b54e302e049a` |
| Collector `scripts/assess_languages.py` Git blob | `f223fe0db778a9163d18167eb8b1a53549333ac0` |
| Successful external correctness push run | [42523766454](https://github.com/buchochelliq-labs/intentumdiff-core/actions/runs/42523766454) |
| CI artifact name / ID | `external-source-correctness-linux` / `13634342616` |
| CI artifact archive SHA-256 | `b54135521dfffe0f245dc09979250be860133690d0175f852cde9ff1ea289ec4` |
| Retained assessment timestamp | `2026-11-08T21:14:16.780571+00:00` |
| This baseline / exact CI summary SHA-256 | `178addef636347d966ab128979f5bcad0139a06d9f207de1d381f45cc654c704` |
| Native C ABI SHA-256 | `7e96a686b4226ea81967685c337bbecbfff42326c3e5a69682e00b7044ca139c` |
| Registry source commit | `c5a909b4fc618c654825de33352d4d5fa816a06a` |
| Immutable archive lock SHA-256 | `f0fadf6201151413ded10eb77df33d8afc4be5b7259b84166b1e534a04d3fa98` |
| Fixed inventory SHA-256 | `37f27f7b6ea8c21b6f7af3f07a189d0c76d73ab8ea316f96eea20ef5559db4d7` |
| Staged parser manifest SHA-256 | `e01020ea30aa7c3bcdf7d2317a8a599e0b2a111692a38379e4b27a2443126a46` |

The successful run is a push of the merged RC after
[Python literal repair #287](https://github.com/buchochelliq-labs/intentumdiff-core/pull/287)
and [verified parser activation #290](https://github.com/buchochelliq-labs/intentumdiff-core/pull/290).
The source tree contains those changes together. Acquisition verifies the
registry-authorized source commits and component hashes against five immutable
CI archives: C/C++, JavaScript/TypeScript, JSON, SQL and YAML. The other 63
components are registry-matched wheel mirrors. The complete source and artifact
identities remain embedded in the JSON's parser manifest.

The summary omits full reviews and parse trees, which remain in the CI artifact's
74 raw worker files. Every raw worker reports the selected native SHA-256 and
all its expected observations. Historical absolute paths and timing values are
retained as run provenance, not portable download paths or acceptance criteria.

The embedded `baseline_gate` records the original CI comparison against 578
protected passes: zero regressions and these six improvements:

- `dynamic/python/language-feature`
- `dynamic/javascript/literal-tail-after-4096`
- `dynamic/typescript/literal-tail-after-4096`
- `data_config.json.known_long_literal_suffix`
- `data_config.sql.known_long_literal_suffix`
- `data_config.yaml.known_long_literal_suffix`

The refreshed gate derives **584 protected passes** from the per-case statuses;
it does not rewrite that historical comparison. Validating the retained raw
workers against this new baseline yields zero regressions and zero additional
improvements. All 760 identities, source hashes and expected verdicts remain
equal to the committed fixtures and both historical baselines.

Use the command in [the assessment guide](../LANGUAGE_ASSESSMENT.md#gate-candidate-changes-against-the-reviewed-baseline)
with this JSON as `--baseline`. Future fixes must retain these successes while
continuing to expose the remaining failures and coverage gaps.
