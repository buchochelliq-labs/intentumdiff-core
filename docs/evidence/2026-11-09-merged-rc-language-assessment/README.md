# Merged RC language replay — 2026-11-09

This is a bounded diagnostic replay of the merged `release/v0.0.3-rc` source at core commit `059fc9cd164c7ade3ff0896e61a7f2cd63fb6f05`, after reorder fix PR #294. It ran the existing 760 source-judged probes across all 74 registered routes against the frozen 584-pass baseline. It is not a full correctness certification of every language.

## Result

- 596 passed, 86 failed, 3 invalid results, 43 bounded incomplete observations, and 32 bounded explicit fallbacks.
- All 584 previously protected passes remain; 12 cases improved and 0 regressed.
- The independent structural audit checked 24,121 source-position nodes, 769 groups, and 600 final references. It found no new source-bound errors, no invalid group references, and no audit issues. Eight pre-existing source-bound errors remain recorded in the historical baseline.
- The audit found 17 stable-review content changes. Seven affect prior passing ADF/Databricks cases; source review found these changes preserve the intended edits while exposing parser structure and more precise source evidence. The ADF scalar-to-array case also reports the unchanged `output` value as a child of the newly added array container.

The changed parser artifacts add configuration-tree context. Their content hashes match the exact ADF and Databricks artifacts pinned in this RC's `external_benchmark_inputs.lock.json`; all other staged components match the reviewed baseline manifest. The exact stage manifest and lock are retained here.

## Reproduction inputs

The native Rust C ABI library was built with the locked host crate using Rust 1.95.0, with debug info and incremental compilation disabled to fit the workspace. Its SHA-256 is in `native-library.sha256`. The parser manifest, benchmark input lock, baseline hash, source commit and tree are retained above. `results.json` and `raw/` contain the 74 route outputs; `audit.json` contains the structural and regression audit.

`audit_adapter.py` adapts the prior audit helper to the new RC native-library hash. It relaxes only the helper's stale host-tree/native-library pin; it retains fixture/oracle identities, the 584-pass baseline, full component-byte checks, source-span checks, and final-reference checks.

## Scope limits

This replay does not close the 86 failed probes, 3 invalid results, incomplete observations, or explicit fallbacks. External source admission and broader construct/span coverage remain open under #261 and #268. The ADF component in this replay is the currently pinned RC artifact and records SDK source commit `db4de224118e5ccbeacc08e80ff0361701d48c8a`; it predates SDK PR #31. Therefore this replay does not validate the decimal-notation fix through a rebuilt ADF component, which remains a separate integration follow-up.
