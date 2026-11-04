# Migration execution ledger
Plan: 2026-09-27-thin-public-apis.md
Baseline: core2e58c19, Python d3fa0d0. Both clean dedicated feature branches in task-specific clones.
Ruling: retain existing stable C ABI and maturin/cffi packaging. Earlier mention of current PyO3 support was historical adapter code; the actual core has already removed PyO3. Rust/Python public APIs remain first-class, future Go/Java remain supported by architecture.
Task ordering: lifecycle and host-utils first provide small public-operation/ABI parity patterns before schema migration; schema remains highest-impact correctness gap.

Resumed from existing dirty migration; preserved all previous work. Scope ruling:
finish #97/#98/#99/#55/#100 and the shared API/corpus gate; broader #39–42 stay open.
Public Python compatibility is retained. Packaging stays maturin/cffi.

Independent research review reproduced explicit-modified empty-file misclassification
and Markdown moves retaining contradictory ignored-style evidence. Added Python tests,
observed all four failures (including C-ABI library incorrectly imported as PyO3), then
fixed causes and observed 4/4 pass. Added native compile-context end-to-end test;
observed missing metadata before native-host discovery integration. Added public Rust
integration consumer, observed missing api import, implemented typed result/error facade.
Added public Python text-review test, observed missing export, supplied thin DTO adapter.

Verification checkpoint: 316 Rust unit tests + 1 external public-API test passed.
Ten source-judged direct native/Python corpus cases passed; evidence under
`docs/evidence/thin-api/actual-diffs.json`. Full Python suite and independent final
review still running; no readiness claim. Initial Python subprocess failures exposed
uninstalled-checkout sys.path, distinct from the repaired backend autodetection.

Independent final review found Unicode columns counted characters and fnmatch classes
interpreted Rust regex set operators. Both were reproduced as failing Python tests,
fixed in Rust, and added to shared corpus/native tests. Latest core suite: 317 unit
+ 1 external API test passed. Source and installed maturin/cffi wheel each passed all
17 corpus comparisons, including explicit expected byte columns and glob matches.
The first full Python run: 2412 passed, 15 failed, 272 skipped, 11 xfailed.
Thirteen failures were subprocess import setup (targeted corrected run passed), one
used an earlier loaded library before native compile wiring (fresh run passed), and
one cannot create AF_UNIX sockets in this execution sandbox. The socket test remains
unchanged for CI. Full rerun uses correct subprocess path and excludes that one test.
