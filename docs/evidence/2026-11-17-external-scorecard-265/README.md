# External-benchmark scorecard: #265 structured family, 2026-11-17

This scorecard adds the core #265 data/schema family to the #268 ledger: `yaml`, `json`, `toml`, `xml`,
`ini`, `proto` and `graphql`. It sits on top of the first HumanEvalPack slice in
`../2026-11-17-external-scorecard/`, which it reproduces unchanged. **It does not certify any language.**
`xml` is BLOCKED in both lanes. No language has a robustness/scaling result.

Files:

- `scorecard.md` / `scorecard.json`: every one of the 74 IDs, with rows per language × suite × class × lane.
- `results.json`: the runner report for all 2,091 admitted cases. This is the new CI regression baseline.
- `parser_manifest.json`: the staged component identities.

## Identity

- Core `bbdec7c9` (crates identical to `74618c31`, clean). The C ABI library is the same release build
  as the first slice, SHA-256 `ced9e0e1…598b`.
- Components: the same staged `build/benchmark-inputs` as the first slice (manifest `23c390a8…`). The
  family's components are `yaml_parser.wasm` `2ff91eb6…`, `json_parser.wasm` `ffb886cf…`,
  `toml_parser.wasm` `af1ec51b…`, `ini_parser.wasm` `dc665e89…`, `proto_parser.wasm` `d692e7aa…`,
  `graphql_parser.wasm` `b01f0a4b…` and `xml_parser.wasm` `2bd8f26b…` (not exercised).
- Transport: intentumdiff-python `0dd7e50`.
- Determinism: a second identical run gave identical statuses, errors and review summaries for all 2,091
  cases. The baseline gate against the first slice exits 0, and its 165 cases keep their statuses.

## Sources (all pinned to a commit; bytes and SHA-256 are in `upstream/structured/<suite>/ACQUISITION.json`)

| Suite | Revision | Licence | Enumeration |
|---|---|---|---|
| YAML Test Suite, `data` branch | `6ad3d2c62885d82fc349026c136ef560838fdf3d` | MIT (`License` at main `da267a5c`) | **incomplete**: no in-suite index. 333 candidate IDs were probed and 385 (sub)tests found. |
| JSONTestSuite `test_parsing` | `1ef36fa01286573e846ac449e8683f8833c5b26a` | MIT | names in `results/parsing.html` (lower bound; the index is hashed, not vendored) |
| JSON Patch Tests | `2a928f9044aad35c74e2788d498bcf2c6b91adea` | Apache-2.0 (README) | `tests.json`, `spec_tests.json` |
| toml-test v1.6.0 | `369cc3ae275fd76b6e21780ebf87609e5b2a951a` | MIT (`COPYING`) | `tests/files-toml-1.0.0` (complete) |
| CPython v3.13.18 configparser | `0d4bd512fc429e3d4eacdd113c975e161f21644d` | PSF-2.0 | `configdata/cfgparser.{1,2,3}` |
| inih | `2bbdec4a366c8c39746ee0982e7ca0febbb044b6` | BSD-3-Clause | `.ini` names in `tests/unittest.c` |
| buf v1.75.0 breaking fixtures | `1a3a05a3e0fa350c4b814b7f48aa0e64e9a906bd` | Apache-2.0 | dirs from `breaking_test.go`; file names probed (`1..9.proto`, `a/b.proto`, `buf.yaml`) |
| graphql-js v16.15.0 | `2b4f68feaed36fd3b221764a467c6fa78a0c9797` | MIT | `findBreakingChanges-test.ts` |

Tags and branches were resolved to commits with the Go module proxy's VCS origin record. Bytes were then
fetched only from `raw.githubusercontent.com/<repo>/<commit>/<path>`.

**Blocked acquisitions:**

- github.com HTML, the REST API and git clones are unreachable for these repositories, so no tree listing
  was possible. YAML and buf enumeration is therefore partial. Two buf cases (`breaking_ignore_unstable_*`)
  need files under subdirectories and stay candidates.
- **W3C XML Conformance Test Suite:** `www.w3.org` refuses the CONNECT tunnel (403). It is recorded in the
  ledger's `blocked_acquisitions`, and `xml` is BLOCKED in both lanes. No other source was substituted.

## Oracles (2,063 executions, `scripts/structured_family.py witness`)

| Oracle | Reference | supports / other |
|---|---|---|
| JSON parse, typed equality | CPython 3.13.18 `json` (strict; NaN/Infinity rejected) | 316 / 0 |
| JSON Patch | `jsonpatch` 1.33 must reproduce the upstream `expected`, then typed equality | 73 / 1 inconclusive (library fails on root `add`) |
| TOML parse, typed equality | `tomllib`; valid files must also equal toml-test's tagged JSON | 623 / 0 |
| YAML parse, typed equality | ruamel.yaml 0.18.16 safe loader (YAML 1.2), compared with `in.json` | 487 / 134 (68 contradict, 66 inconclusive) |
| INI | configparser under each upstream test's options and assertions; inih's own `unittest.c` built with gcc 13.3 and compared byte-for-byte with `baseline_multi.txt` | 22 / 0 |
| proto | buf 1.75.0 `build`; `breaking` must reproduce the exact upstream annotation set; image equality without source info | 309 / 2 inconclusive |
| graphql | graphql-js 16.15.0 `buildSchema`; `findBreaking/DangerousChanges` must equal the upstream assertions; sorted `printSchema` equality | 96 / 0 |

Only `supports_claim` admits a case. Where the YAML reference disagrees with the suite label, the case
stays a candidate. That biases the admitted YAML subset toward inputs ruamel handles.

## Results (admitted cases through the C ABI)

| Language | Lane | Class | Passed / executed | Notes |
|---|---|---|---|---|
| json | parser | upstream_gold | 97/245 | valid accepted 95/95; **invalid rejected 2/150** |
| json | change | reviewed_external_derived | 128/144 | change recall 98/100; preservation false alarms 14/44 |
| toml | parser | upstream_gold | 270/546 | valid 185/185; **invalid rejected 85/361** |
| toml | change | reviewed_external_derived | 72/77 | recall 48/48; preservation failures 5/29 (all `incomplete`) |
| yaml | parser | upstream_gold | 270/303 | valid 229/229; invalid rejected 41/74 |
| yaml | change | upstream_gold (in→out) | 46/172 | preservation false alarms 126/172 |
| yaml | change | reviewed_external_derived | 6/12 | recall 6/6; comment false alarms 6/6 |
| ini | parser | upstream_gold | 9/9 | accept-only (no dialect-neutral reject rule) |
| ini | change | reviewed_external_derived | 13/13 | |
| proto | parser | upstream_gold | 173/185 | 12 valid files rejected by explicit fallback |
| proto | change | upstream_gold | 93/93 executed (+7 unsupported) | buf pairs |
| proto | change | reviewed_external_derived | 24/24 | |
| graphql | parser | upstream_gold | 46/46 | |
| graphql | change | upstream_gold | 14/25 | **11 schema changes reported as style-only** |
| graphql | change | reviewed_external_derived | 25/25 | |

## Real failures, minimised (not filed)

Each reproducer was rechecked directly through `live_diff_contents`/`parse_to_tree`:

1. **GraphQL field types are not represented.** `type Query {\n  foo: Float\n}` → `foo: String` gives
   `has_semantic_changes=false, is_style_only=true`. The tree has `field 'foo'` with no type, so nullability
   and type changes are invisible.
2. **GraphQL single-line type bodies lose their fields.** In `type Query { foo: Float }`, the tree is
   `type_definition 'Query'` with no children, so even a field rename is style-only.
3. **JSON numbers with an explicit `+` exponent are truncated.** `[1e+22]` parses as `number '22'` and
   `[1e+2]` as `number '1'`. Both `[1e+22]`→`[2e+22]` and `[1e+2]`→`[1e+3]` are style-only.
4. **JSON/TOML/YAML accept invalid input silently.** `[1 true]` is parsed as `array[true]` with no
   diagnostics: the `1` is dropped, and `[1 true]`→`[2 true]` is style-only. TOML `a = [1,,2]` and YAML
   2CMS (`this\n is\n  invalid: x`) give no `parse_errors` and no fallback. 148 of 150 JSON, 276 of 361
   TOML and 33 of 74 YAML must-reject documents are affected.
5. **YAML double-quoted scalars with an escape lose their text.** `"a\/b"` is labelled `\/`, and
   `k: "a\nb"` → `k: "c\nd"` is style-only (a value change missed).
6. **YAML comments become node labels.** `a: 1` → `# c\na: 1` reports `MODIFICATION stream 'stream' → '# c'`,
   and a trailing comment reports `block_mapping → '# c'`. Quoting style is compared lexically:
   `k: a` → `k: 'a'` is a MODIFICATION. Together these cause most of the 126 in→out false alarms; 28 more
   are `incomplete` reviews.
7. **proto fallback on valid files (buf build succeeds).** Two inputs trigger it:
   `syntax = "proto2";\nmessage A {\n  optional group Foo = 1 {\n  }\n}` and the edition-2023 option
   `string s = 1 [features.(pb.cpp).string_type = VIEW];`.
8. **JSON re-serialisation false alarms.** Number respelling (`1E22`→`1e+22`) and escape
   normalisation are reported as semantic MODIFICATIONs, even though the typed values are equal.
9. **TOML `8080` → `8_080`** returns `completeness: incomplete` and is not proven equivalent.

## Not claimed

- XML is not covered: its acquisition is blocked. No lane has robustness/scaling cases.
- INI must-reject behaviour is not admitted, because the route declares no dialect.
- YAML enumeration is partial, and the admitted YAML subset is filtered by agreement with ruamel.
- Compatibility labels (buf, graphql-js) are one semantic dimension, not author intent.
- Derived edits are bounded at 24 per derivation and suite, and are built deterministically from sorted seeds.
