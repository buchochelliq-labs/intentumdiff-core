# External-benchmark scorecard: #267 build, configuration and infrastructure family, 2026-11-17

This run adds the core #267 IDs to the #268 schema-2 ledger: `dockerfile`, `hcl`, `gomod`, `gitignore`, `cmake`,
`make` and `puppet`. **It does not certify any language.** `bash` and `powershell` are scripting languages and are
left to #266; this family adds no cases for them.

The family adds 776 cases, of which 761 are admitted and executed through the C ABI: 622 passed, 128 failed and
11 unsupported. Every one of the seven IDs now has a measured parser-fidelity lane and a measured
change-understanding lane with witnessed changed and preserved pairs. No ID has a robustness/scaling result.

Files:

- `results.json`: the runner report for all 2,894 admitted cases of the ledger (the 2,133 of the combined
  first-slice/#262/#265 run plus the 761 #267 cases).
- `scorecard.md` and `scorecard.json`: rows for each language × suite × class × lane.
- `parser_manifest.json`: the staged component identities (the same manifest as the combined run).

## Run

- Worktree `bench-267` on `release/v0.0.3-rc` `8898e3f`. `crates/` is identical to `74618c31`, so the C ABI
  library is the same release build as every earlier run, SHA-256 `ced9e0e1…598b`.
- Components: the same staged `build/benchmark-inputs` (manifest `23c390a8…`). The family's components are
  `dockerfile_parser.wasm`, `terraform_parser.wasm` (route `hcl`), `gomod_parser.wasm`, `gitignore_parser.wasm`,
  `cmake_parser.wasm`, `make_parser.wasm` and `puppet_parser.wasm`; their hashes are in `parser_manifest.json`.
- Transport: intentumdiff-python `0dd7e50` on `PYTHONPATH`, Python 3.12.3.
- Ledger SHA-256 `1fe0ffce…803c`.
- **Determinism.** Two full runs gave identical statuses, errors, worker statuses, completeness and review
  summaries for all 2,894 cases. The witness step was also run twice; all 776 witness records were byte-identical.
- **Unchanged statuses.** All 2,133 cases of `../2026-11-17-external-scorecard/results.json` have the same status,
  errors and review summary. The gate exits 0 with that report as `--baseline`.

## Sources

Every suite is pinned to a commit. Bytes and SHA-256 are in `upstream/build-infra/<suite>/ACQUISITION.json`,
written by `scripts/acquire_build_infra_benchmarks.py`. Two methods were used:

- **Go module zips.** The tool runs `go mod download -json`, which verifies the zip against sum.golang.org.
  The module's VCS origin gives the commit.
- **Raw files.** Tags and branches were resolved with the Go module proxy's VCS origin record. The files were then
  fetched from `raw.githubusercontent.com/<repo>/<commit>/<path>`.

| Suite (language) | Revision | Licence | Acquisition |
|---|---|---|---|
| BuildKit v0.17.3 `frontend/dockerfile/parser` testfiles (`dockerfile`) | `dc30e8355a4691bb34a23dd7161306977eecd5d8` | Apache-2.0 | module zip `af50411c…`, `h1:XN8ddC5g…` |
| HCL v2.23.0 `specsuite/tests` (`hcl`) | `56a9aee5207dbaed7f061cd926b96fc159d26ea0` | MPL-2.0 | module zip `78c8c335…`, `h1:Fphj1/gC…` |
| Terraform v1.5.7 `internal/configs/testdata/{valid,invalid}-files` (`hcl`) | `ee58ac1851c8a433005df9863ed47796a9f6b5e7` | MPL-2.0 (v1.5.7 predates the BUSL change) | module zip `dbac9050…`, `h1:HL+iEBdp…` |
| golang.org/x/mod v0.22.0 `modfile/testdata` (`gomod`) | `dec0365065b75edd0e98b0306f6f9b0051710ed2` | BSD-3-Clause | module zip `0d0a9605…`, `h1:D4nJWe9z…` |
| Go go1.24.7 `src/cmd/go/testdata/mod` and `script/mod_replace.txt` (`gomod`) | `d41a8222133aabaf5527eab64586050a0fbb773b` | BSD-3-Clause | raw; the 251 testdata/mod files equal the local go1.24.7 GOROOT byte for byte |
| git v2.43.0 `t/t0008-ignores.sh` (`gitignore`) | `564d0252ca632e0264ed670534a51d18a689ef5d` | GPL-2.0 | raw |
| CMake v3.28.3 `Tests/RunCMake/Syntax` and `RunCMake.cmake` (`cmake`) | `5e984bb35232116a54de7db39788cb162ca7c263` | BSD-3-Clause | raw; enumerated from `RunCMakeTest.cmake` |
| GNU Make 4.3 `tests/` (11 scripts plus the harness) (`make`) | `f430a65ccb5e85eb465f6a64afcbdd09f9c708ed` (mirror/make) | GPL-3.0-or-later | raw |
| puppet `spec/unit/pops/parser` (9 specs plus the helper) (`puppet`) | `e227c27540975c25aa22d533a52424a9d2fc886a` (main) | Apache-2.0 | raw; all 11 files are identical at tag 8.10.0 |

**Licences.** git's test script (GPL-2.0) and GNU Make's tests (GPL-3.0-or-later) are vendored as unmodified test
data, with COPYING retained and attribution on every case. The ignore files and makefiles extracted from them keep
the same licence and attribution. None of these bytes are linked into or shipped with IntentumDiff. That is the
basis on which they were vendored; the case records state it as well.

**Pins.** The puppet tag `8.10.0` could not be resolved to a commit, because proxy.golang.org returned not found for
it. The pin is therefore the `main` commit above, and every vendored file was compared byte for byte with the same
path at the tag. The tool used as the oracle is the released gem 8.10.0
(sha256 `f2f5de8f…a3a`, as listed by rubygems.org).

## Oracles (776 witness records, `scripts/build_infra_witness.py witness`)

| Oracle | Tool and version | supports / other |
|---|---|---|
| Dockerfile parse | BuildKit v0.17.3 `parser.Parse`. Must-accept files must reproduce the upstream `result` dump byte for byte. Must-reject files must return an error. | 36 / 0 |
| Dockerfile pairs | BuildKit `instructions.Parse` typed command lists, without locations, original text, the legacy-form marker or keyword spelling | 15 / 1 inconclusive (`health`: the seed's bare `HEALTHCHECK` fails `instructions.Parse`) |
| HCL parse | hcl v2.23.0 `hclsyntax.ParseConfig`. valid-files and specsuite files that expect no diagnostics must be accepted. | 62 / 0 |
| HCL pairs | the hclsyntax information model: attributes as an unordered map of cty-typed constants, or token text for other expressions; blocks as an ordered sequence. `hclwrite.Format` (the formatter behind `terraform fmt`) generated three canonical-format pairs. | 19 / 0 |
| go.mod parse | x/mod v0.22.0 `modfile.Parse` (strict) and cmd/go go1.24.7 `go mod edit -json` | 139 / 10 contradict (5 x/mod format-only fixtures such as `block`, `comment` and `rule1` are not valid go.mod; 5 cmd/go testdata members are intentionally bad) |
| go.mod pairs | `go mod edit -json` equality | 17 / 2 inconclusive (`block`, `comment` goldens do not parse) |
| gitignore | git 2.43.0. The setup files reproduce t0008's `--stdin -v` expected-verbose output. The `**` file reproduces its `check-ignore` expectation, and the backslash file its `ls-files -X` expectation. Pairs compare `check-ignore --no-index -z --stdin` sets over a fixed universe of materialised files. | 19 / 0 |
| cmake | the upstream RunCMake harness on cmake 3.28.3 (Unix Makefiles). Pairs compare the configure exit status, stdout and stderr with paths and timings normalised. | 118 / 2 inconclusive (`AtWithVariable*File` need an input file that is not vendored) |
| make | the upstream `run_make_tests.pl` harness on GNU Make 4.3 and Perl 5.38.2; it reproduced all 70 makefile expectations. Pairs compare `make` exit status, stdout and stderr. | 78 / 0 |
| puppet | puppet 8.10.0 (Ruby 3.3.6) `Parser#parse_string`, which is what the specs' `parse` helper calls. Pairs compile a catalog for a fixed node and normalise resources plus ordering/refresh edges. | 258 / 0 |

Only `supports_claim` admits a case. Two rules were adjusted after the first witness run, before any engine run:

- **Keyword spelling.** The Dockerfile pair projection first included the instruction keyword as written, so `ADD`
  → `add` contradicted the claim that instructions are case-insensitive. BuildKit dispatches keywords
  case-insensitively, so the spelling is no longer observed. The `type` field still identifies the command.
- **Expansion-time make errors.** The make must-reject rule first counted `unterminated variable reference`, which
  GNU Make raises when a variable is expanded, not when the line is read. Executed check:
  `x = $(foo` / `all: ; @echo hi` prints `hi` and exits 0 on make 4.3. `variables/negative` tests 0 and 1 are
  therefore must-accept.

### Parser-fidelity claims

- **dockerfile:** BuildKit's own must-accept and must-reject split. `testfiles-negative/empty_dockerfile` and
  `only_comments` are excluded. Their only upstream error is "file with no instructions", which describes an empty
  document, not a syntax error.
- **hcl:** native-syntax acceptance. Terraform invalid-files whose error is a schema or semantic error are valid HCL
  and must be accepted; those with syntax errors must be rejected. `.tf.json` files are not routed to `hcl`.
- **cmake:** must-reject when the upstream expected stderr reports `Parse error` (or `Syntax Error in cmake code`)
  in the file. Configure-time errors such as an unmatched `function()` or `${` in an argument leave the file
  must-accept.
- **make:** must-reject when the upstream expected output is a makefile read error, such as a missing separator, an
  empty variable name or a missing `endef`.
- **puppet:** the spec's own expectation, either `expect(...)` success or `expect { parse(...) }.to raise_error`.
- **Statement inventories:** only Dockerfile, HCL and go.mod documents without comments carry one, taken from the
  reference's unit start bytes. The comparison domain cannot tell a comment node from a statement without node type
  names.

## Cases per language × lane × class (admitted / executed / passed / failed / unsupported)

| Language | Lane | Class | Admitted | Passed | Failed | Unsupported | Candidates |
|---|---|---|---:|---:|---:|---:|---:|
| dockerfile | parser | upstream_gold | 36 | 18 | 18 | 0 | – |
| dockerfile | change | reviewed_external_derived | 15 | 6 | 0 | 9 | 1 |
| hcl | parser | upstream_gold | 62 | 55 | 7 | 0 | – |
| hcl | change | reviewed_external_derived | 19 | 16 | 3 | 0 | – |
| gomod | parser | upstream_gold | 139 | 137 | 2 | 0 | 10 |
| gomod | change | upstream_gold (x/mod .in→.golden) | 6 | 1 | 4 | 1 | 2 |
| gomod | change | reviewed_external_derived | 11 | 9 | 2 | 0 | – |
| gitignore | parser | reviewed_external_derived (here-document payloads) | 7 | 7 | 0 | 0 | – |
| gitignore | change | reviewed_external_derived | 12 | 7 | 5 | 0 | – |
| cmake | parser | upstream_gold | 108 | 71 | 37 | 0 | 2 |
| cmake | change | reviewed_external_derived | 10 | 6 | 3 | 1 | – |
| make | parser | upstream_gold | 60 | 54 | 6 | 0 | – |
| make | parser | reviewed_external_derived (harness-substituted) | 10 | 10 | 0 | 0 | – |
| make | change | reviewed_external_derived | 8 | 5 | 3 | 0 | – |
| puppet | parser | upstream_gold | 235 | 197 | 38 | 0 | – |
| puppet | parser | reviewed_external_derived (escaped literals) | 16 | 16 | 0 | 0 | – |
| puppet | change | reviewed_external_derived | 7 | 7 | 0 | 0 | – |

### Results per language × lane

| Language | Lane | Valid accepted | Invalid rejected | Change recall | Preservation false alarms |
|---|---|---|---|---|---|
| dockerfile | parser | 16/34 | 2/2 | N/A | N/A |
| dockerfile | change | N/A | N/A | 3/3 | 0/3 (9 unsupported) |
| hcl | parser | 53/55 | 2/7 | N/A | N/A |
| hcl | change | N/A | N/A | 9/9 | 3/10 |
| gomod | parser | 137/139 | N/A | N/A | N/A |
| gomod | change | N/A | N/A | 6/7 | 5/9 |
| gitignore | parser | 7/7 | N/A | N/A | N/A |
| gitignore | change | N/A | N/A | 6/6 | 5/6 |
| cmake | parser | 65/88 | 6/20 | N/A | N/A |
| cmake | change | N/A | N/A | 2/4 | 1/5 |
| make | parser | 64/65 | 0/5 | N/A | N/A |
| make | change | N/A | N/A | 1/4 | 0/4 |
| puppet | parser | 212/234 | 1/17 | N/A | N/A |
| puppet | change | N/A | N/A | 3/3 | 0/4 |

## Real failures, minimised (recorded, not filed)

Each reproducer was rechecked directly through `live_diff_contents`, using the same transport and library as the
runner.

**dockerfile (`dockerfile_parser.wasm`)**

1. **Legacy forms fall back.** Each of these gives `native_parse_incomplete`, while `LABEL a=b` and `ENV a b` are
   fine:
   - the legacy `LABEL a b`;
   - the legacy `ENV a b c` with a multi-word value;
   - `ARG a b`, which declares two names;
   - a trailing continuation at EOF (`RUN a \` with no newline);
   - `ENV name='a\'b''`.

   This is 16 of the 34 must-accept upstream files and all 9 unsupported change cases (their seeds contain a
   legacy `LABEL` or `ENV`).
2. **Instructions are dropped or split.** BuildKit treats an unterminated JSON array
   (`CMD ["echo", "Please, close the brackets when you're done"`) as shell form, but the tree has no instruction
   node. In `health`, the multi-line `HEALTHCHECK … \` continuation yields 12 units against BuildKit's 10.

**hcl (`terraform_parser.wasm`)**

3. **Identity invents changes for repeated blocks.** With the identical input `a {\n  x = 1\n}\na {\n  x = 2\n}\n` on
   both sides, the attributes are cross-matched and reported as a MODIFICATION of `x` (Terraform `provider-configs.tf`
   and `providers-explicit-implied.tf`).
4. **Unclosed and invalid syntax is accepted silently.** `a {` reviews as style-only with no diagnostics. The same
   happens to `version = ` with no value and to `${…}` interpolation in a block label.
5. **Reorders are not equivalent.** Reordering an object constructor's keys (`{"a" = 1, "b" = 2}` → `{"b" = 2,
   "a" = 1}`) is reported as a MODIFICATION of the attribute. Swapping two attributes (`a = 1\nb = 2` → `b = 2\na = 1`)
   returns `completeness: incomplete`.

**gomod (`gomod_parser.wasm`)**

6. **`// indirect` is missed.** `require a.b/c v1.0.0` → `require a.b/c v1.0.0 // indirect` is style-only, but cmd/go
   reports `Indirect: true`.
7. **Quoting is a MODIFICATION.** `module "x"` → `module x` and `replace "a.b/c" v1.0.0 => "/tmp/z"` → the unquoted
   form are reported as MODIFICATIONs of `module_path`/`file_path`, although `go mod edit -json` is equal. Four of
   the five x/mod `.in`→`.golden` format pairs fail this way (`module`, `retract`, `replace2`; `empty` is a
   completeness mismatch).
8. **Fallback on quoted paths.** `replace (\n\tw v1.0.0 => "./a,"\n)` falls back on the valid x/mod
   `replace.in`/`replace.golden`.

**gitignore (`gitignore_parser.wasm`)**

9. **Comment lines are code edits.** Adding `# c` before `one` is a semantic ADDITION of a `comment` node, and
   removing comment lines is a DELETION (3 false alarms).
10. **Reordering returns `incomplete`.** Swapping two patterns with disjoint matches (`one\ntwo` → `two\none`), or
    two negations, returns `completeness: incomplete`.

**cmake (`cmake_parser.wasm`)**

11. **Parse errors are accepted silently.** 14 of 20 must-reject files are affected. Minimal inputs:
    - `message("Example Message") message("Second Message")` (no separator between commands);
    - `message(STATUS [[bracket]]unquoted)`;
    - `CommandEOF`, `ParenNoSpace1`, `UnterminatedBracketComment` and `NullTerminatedArgument`.
12. **Identity invents changes.** `message(${var with space})` and `message("-->${e(x)}<--")` on both sides report an
    ADDITION of `normal_command` labelled `message({var)` / `message(-->$)` (10 files, among them the
    must-reject `CommandError0`).
13. **Fallback on files CMake parses.** Several files fall back although CMake parses them:
    - `function(f)` or `foreach(i 1 2)` without its end command;
    - `set(var "${")` and `message("${var with space}")`;
    - `Bracket1`'s multi-line `[=[…]==]=]`;
    - `CMP0053-NUL`.

    CMake reports configure-time errors for some of these, not parse errors.
14. **Missed value changes.** `message(a " " b)` → `message(a "  " b)` is style-only, and so is
    `message("a \${v} 4")` → `… 5`, which has an escaped `$` in the string. Both change the output.
15. **Command case is a MODIFICATION.** `message(a)` → `MESSAGE(a)` is reported as a MODIFICATION, but command names
    are case-insensitive.

**make (`make_parser.wasm`)**

16. **Flavor changes are missed.** These are reported as style-only with `completeness: equivalent`:
    - `X = $(Y)` → `X := $(Y)` (recursive to simple);
    - `foo := $(bar)` → `foo = $(bar)`;
    - `foo += $(bar)` → `foo = $(bar)`.

    GNU Make prints different output for each.
17. **Read errors are accepted silently.** All 5 must-reject makefiles review without diagnostics: `$x.` (missing
    separator), 8 spaces instead of a tab, an empty variable name and an unterminated `define`.
18. **Fallback on a valid conditional.** `variables/define` test 3 falls back on valid input. It contains
    `ifdef BOGUS` around `define $(subst e,e,$(NAME)) =`, with an `ifeq`/`endif` in the body, and GNU Make prints
    `foo`. Smaller variants do not reproduce it, so the 14-line file is the reproducer.

**puppet (`puppet_parser.wasm`)**

19. **Valid manifests fall back (22 of 234).** Minimal examples:
    - `! 'x'`, `[]`, `$a = [1,2,3][2]`, `$a -= 10` and `$a.foo`;
    - `unless true { $a = 10 } else {$a = 20}`;
    - `@File[foo] { mode => '0777' }`;
    - `class { 'cname1': x => 1; 'cname2': y => 2}`;
    - `File <| tag == 'foo' and mode != '0777' |>`.
20. **Invalid manifests are accepted (16 of 17).** Examples are `class class {}`, `type A = B[]`,
    `notice a => 2` and `@file {: mode => '0777' }`.

## Blocked or left as a gap

- **IaC-Eval (`hcl`, the selected primary source): BLOCKED.** huggingface.co refuses the CONNECT tunnel (403),
  so no revision-pinned bytes could be hashed. The dataset card (read through the Hugging Face MCP) says CC-BY-4.0.
  It is recorded in `blocked_acquisitions`. HCL is measured from the HCL spec suite and Terraform testdata.
- **Shipwright (`dockerfile`, the selected primary source): BLOCKED.**
  - No commit could be resolved: `git ls-remote` was refused by the permission policy, github.com and its API are
    unreachable, and the Go proxy returns not found.
  - Its build oracle needs a Docker daemon, which is not available.
  - It is recorded in `blocked_acquisitions`. Dockerfile change cases are BuildKit-derived instead.
- **No image builds.** There is no Docker daemon and BuildKit has no worker, so Dockerfile change pairs are judged
  at the parsed-instruction level only. No image config or runtime is observed.
- **No `terraform validate`/`plan`.** Providers would need registry.terraform.io, and no Terraform binary was
  installed. HCL claims are syntax and information-model claims, not Terraform semantics.
- **gomod:** `go build`/`go run` witnesses (the mod_replace runtime output) were not run, because they need the
  module proxy test server. The 2 `block`/`comment` golden pairs and 10 invalid or format-only documents stay
  candidates.
- **cmake:** `AtWithVariableFile`/`AtWithVariableAtOnlyFile` need an input file that is not vendored, so they stay
  candidates. Script-mode (`-P`) tests and other RunCMake directories are not imported.
- **make:** only 11 scripts are imported. Expected outputs were reproduced as root on Linux.
- **puppet:** `compiler_spec.rb` catalog specs and the OOPSLA 2024 fault corpus were not imported. Interpolated and
  heredoc spec literals are not extracted. Change pairs compile catalogs only, and no provider is run.
- **bash/powershell:** left to #266.
- **Robustness/scaling lane:** no admitted cases for any of the seven IDs.

## Not claimed

- Instruction-list, information-model, catalog or output equality on the exercised inputs is not program or
  build equivalence.
- Derived pairs are reviewed transformations of upstream files, not upstream edit benchmarks.
- No pooled or macro average is computed.
