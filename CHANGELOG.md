# Changelog

## Unreleased — v0.0.3 correctness foundation

- Windows ARM64 admits the registry #20 PowerShell component (`b47cff15…`, PowerShell
  parser PR #9), which that parser's CI compiled on real Windows ARM64 with fuel-metered
  Wasmtime 48.0.7 (run #42994016353). The earlier PR #5 component stays admitted; every
  other PowerShell payload is still refused on that platform.
- Every served review now carries `completeness` (`complete` | `equivalent` | `incomplete`)
  and `incomplete` (core #252). Differing sources whose drafts were all suppressed without an
  equivalence proof are `incomplete`, never a clean review: the DTO keeps `changes=[]`, adds
  changed byte/line ranges and a core-authored report-a-bug `message` with a stable `reason`
  and `report_url`. Identical input is `complete`; proved style-only input is `equivalent`.
  Binding authors: render `incomplete.message` verbatim (see `docs/C_ABI.md`).
- An `incomplete` review now carries `incomplete.report = {title, body_markdown, truncated,
  issue_url, outline}`: a core-built, sanitised repro outline (core/language/plugin
  versions, node-kind skeletons and shapes, the passes that removed drafts, line/column
  offsets and byte sizes; identifiers as `id1…`, literals as type and length, comments
  dropped). `report_url` is the pre-filled GitHub issue URL, capped at 8000 bytes with
  deterministic truncation. Bindings MUST preview it and require explicit confirmation
  before opening the URL; core never sends anything.
- Member reorders inside JSON objects, YAML mappings and TOML tables are proved equivalent
  (`metadata.equivalence_proof = keyed_data.unordered_mapping_member_order`); duplicate or
  escaped keys, YAML anchors/aliases/tags and TOML arrays of tables decline the proof.
- A `number` leaf respelled to the same exact decimal (`9` → `9.0`, `1000` → `1e3`,
  `1E-2` → `0.01`) in JSON, ADF and Databricks is an equivalent respelling
  (`keyed_data.number_exact_decimal_respelling`): the pair leaves the review as an
  `IGNORED_STYLE` group showing both spellings as written, and a review it empties is
  `equivalent` with that `metadata.equivalence_proof` (`is_style_only=false`). Only strict
  RFC 8259 tokens within the plugin-sdk #36 exponent domain are proved; `1_000`, `0x10`,
  `+1`, `.5`, `01`, `-0` vs `0`, out-of-range exponents and number↔string edits stay
  meaningful. `apply_invariances` results now carry `equivalence_proof`.
- The certified native Python route ids operator leaves as dotted child-index paths
  (`x = 1`: `=` is `.1`, `1` is `.2`), the plugin SDK semantic-token contract the Python
  component now emits, instead of a native-only `<parent>.op<k>` that shifted every later
  sibling. The certified and a custom Python component route agree on node ids again
  (intentumdiff-python#346). Host recovery for components that still drop operators keeps
  its `.op<k>` names.
- A reference swap between variables that already exist (`max_depth -= 1` → `depth -= 1`,
  with both names still read) is a behaviour-changing MODIFICATION, never a
  `RENAME_VARIABLE` refactoring (core #316). A local binder or read counts as renamed only when
  its new name is new to the enclosing callable (or file) and the old name is gone from it;
  consistent renames to a fresh name are unchanged.
- A same-label MODIFICATION survives when an id-aligned inner node's own label changed. This
  covers JavaScript `augmented_assignment_expression`, which carries its target name in its
  label rather than in a leaf. Previously HumanEvalPack 06's JavaScript repair surfaced only as
  a column-shift MOVE of the literal `1` (core #316).
- Retitling a LaTeX section (`\section{Old}` → `\section{New}`) or an AsciiDoc section is a
  MODIFICATION ("Change section title …") in a meaningful group: the heading text is content,
  not a symbol, so it no longer carries a `RENAME_SYMBOL` refactoring claim (core #166). INI
  sections and code entities keep their rename handling.
- Incomplete input mid-edit (core #314): when a parser component returns its in-band error
  for a side, the live and native review declines with `fallback_code: "plugin_parse_error"`
  and keeps the parser's message (`plugin returned error (new): Empty semantic tree`).
  `source_fallback_diff(..., reason="plugin_parse_error")` keeps the edit as a parse error.
  New C ABI operation `plugin_parse_error_review` for bindings that host the parser
  themselves: core decides between that typed source fallback (with the parser's message
  and side) and a typed decline when `fallback_to_token_diff` is off.
  The eight assessment cases that used to report `literal enrich ... missing field id` now
  decline with the parser's own reason; the misleading decode error was removed in #280.
- The language assessment checks the completeness state; cases may pin
  `expected.completeness`.
- Add a source-pinned six-language external pilot, explicit admission ledger for
  all 74 routed IDs, independent control expectations and machine-readable results.
- Add an offline Rust public-API benchmark gate and exact installed-binding
  acceptance, separating semantic accuracy from parity and infrastructure failures.
- Add the core #268 schema-2 admission ledger (`tests/fixtures/external/admission/`), its
  validator and per-language scorecard (`scripts/external_ledger.py`), executed upstream-test
  witnesses (`scripts/humanevalpack_witness.py`), a deterministic ledger builder and a C ABI
  runner reusing `assess_languages.py`. First slice: 165 admitted HumanEvalPack and
  literal cases; scorecard in `docs/evidence/2026-11-17-external-scorecard/` with every
  registered ID (68 gaps) and four recorded failures. CI guards upstream_gold regressions.
- Admit the #262 SQL and data-platform family to the #268 ledger: PostgreSQL regression
  (`sql`), SqlScriptDOM (`tsql`), powerquery-parser (`m`) and Azure Data Factory samples
  (`adf`), plus utPLSQL seeds (`plsql`); 42 admitted cases witnessed by PostgreSQL 16.15,
  SQLite, ScriptDom and powerquery-parser (`scripts/data_family_witness.py`), with statement
  inventories for the parser-fidelity lane. SQLLogicTest and the ADF schema are blocked.
- Admit the #265 structured family: `yaml`, `json`, `toml`, `ini`, `proto` and `graphql`;
  1,926 admitted cases from pinned upstream suites (YAML Test Suite, JSONTestSuite, JSON
  Patch Tests, toml-test, CPython configparser, inih, buf breaking fixtures, graphql-js),
  witnessed by reference implementations (`scripts/acquire_structured_benchmarks.py`,
  `scripts/structured_family.py`). The W3C XML suite is a recorded blocked acquisition, so
  `xml` is BLOCKED in both lanes, never a gap or a pass.
- Admit the #263 web, markup and embedded family: `markdown`, `mdx`, `html`, `css`, `scss`,
  `vue`, `svelte`, `astro`, `latex`, `asciidoc` and `po`; 2,160 admitted cases from pinned
  upstream suites (CommonMark and GFM specs, WPT html5lib tree construction, css-parsing-tests,
  sass-spec, Svelte, Vue compiler-sfc and Astro compiler tests, MDX docs, Asciidoctor DocTest,
  polib fixtures, LaTeX2e documents), witnessed by reference renderers and compilers
  (`scripts/acquire_markup_benchmarks.py`, `scripts/markup_family.py`). `postscript` (licence)
  and `generic` (WikiAtomicEdits unreachable) are recorded blocked acquisitions.
- Admit the #267 build, configuration and infrastructure family: `dockerfile`, `hcl`,
  `gomod`, `gitignore`, `cmake`, `make` and `puppet`; 761 admitted cases from pinned
  upstream suites (BuildKit parser testfiles, HCL specsuite and Terraform testdata, x/mod
  and cmd/go testdata, git t0008, CMake RunCMake Syntax, GNU Make tests, puppet parser
  specs), witnessed by BuildKit, hclsyntax, `modfile.Parse`/`go mod edit -json`, git,
  CMake, GNU Make and puppet (`scripts/acquire_build_infra_benchmarks.py`,
  `scripts/build_infra_witness.py`, code module `family_267_build_infra.py`). IaC-Eval and
  Shipwright are recorded blocked acquisitions; `bash` and `powershell` are left to #266.
- The ledger builder merges family contributions in a fixed order: code modules under
  `scripts/external_families/`, then generated fragments under `admission/families/`. One
  parser-fidelity adapter judges `expected.parse` must-accept / must-reject claims and
  reference statement inventories; parser-fidelity rows score valid-acceptance and
  invalid-rejection rates, not change facts.
- One combined scorecard for the first slice, #262, #265, #263 and #267 (5,054 admitted
  cases: 4,129 passed, 906 failed, 19 unsupported; 40 IDs without a measured lane) in
  `docs/evidence/2026-11-17-external-scorecard/`, measured on the registry #21 components;
  the separate -265 and -267 runs are folded into it. It is the CI regression baseline, and
  CI also runs `structured_family.py build --check` and `markup_family.py build --check`
  before `build_external_admission.py --check`, which re-runs the #262 and #267 code modules.
- Consume the verified C/C++ component preserving character-literal payloads,
  prefixes, escapes and byte spans. Wider 0.0.3 acceptance remains open.

## Unreleased — v0.0.2 release candidate

- Preserve callable rename identity alongside independent body edits and statement
  reorders, including insertion of a sibling helper. Require unambiguous body and
  enclosing-scope evidence; keep meaningful body groups separate from rename groups.
- Add native and routed regression evidence for Python, plus public-wrapper
  JavaScript/TypeScript checks.
- Prefer exact source/body evidence for callable continuity, reject ambiguous positional
  fallbacks, and preserve Python decorator order through sibling insertion and formatting.
- Recognize bounded Python expression-to-helper extraction while retaining unrelated edits.
- Move active Python literal enrichment and tree-equivalence evaluation behind the C ABI;
  preserve UTF-8 source spans and literal whitespace. Limit CSS color equivalence to values.

- Preserve exact callable renames across parser signature shapes and opaque declarations,
  while retaining ambiguity protections. Keep nested CSS custom-property tokens meaningful.

## v0.1.0 — 2026-07-26

Initial import from the IntentumDiff monorepo (files-only; the monorepo remains the archive of
record). The complete engine: semantic diff pipeline across 69 languages, the stable C ABI
(`intentumdiff_call`), the native `intentumdiff` CLI, the index engine, and the four renderer
components. Engine pin suite 203/203.
