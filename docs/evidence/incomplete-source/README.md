# Incomplete-source evidence (core #21)

Python was audited first: its fallback split whitespace and ran SequenceMatcher.
The direct fallback lost indentation and spaces inside unfinished strings. Five
regressions failed against that baseline; the Rust C ABI test failed because the
shared operation did not yet exist. The migration preserves exact byte ranges,
uses bounded previews, and leaves semantic equivalence explicitly unknown.

## Verification

- Rust host suite: 272 passed, 25 parser-dependent tests ignored (`--no-default-features`).
- Python source regression set: 125 passed, 1 existing skip (incomplete-source,
  engine boundary, adapter, rename/body-edit and literal/CSS boundary tests).
- Rebuilt development wheel, installed and tested outside the checkout: 83 passed
  (incomplete-source, rename/body-edit and semantic-boundary files).
- Independent agent: 200 deterministic randomized Unicode/CRLF/whitespace range
  reconstruction checks, additional public native/routed cases under Rust-only,
  native live partial signatures, addition/deletion, identical invalid source and
  recovery to valid syntax. It confirmed Rust ownership and reviewed the native
  Wasm consumer's preservation of the complete fallback payload.
- The reviewer identified no-change syntax metadata and parser provenance mistakes;
  both were corrected and independently retested before publication.

The full Python unit run was attempted and stopped at
`test_every_advertised_format_produces_output[patch]`: the restored local artifact
set has no patch renderer. It reported 9 passed, 1 failed, 8 deselected before
stopping. This is not a full-suite certification. Fresh CI is required. Local
build warnings include existing unused Rust functions and maturin's missing cffi
package-dependency warning; the product boundary itself uses ctypes.

## Actual CLI evidence

`cli.txt` is stdout captured from the installed wheel, run from `/tmp` (outside
both checkouts), comparing `old.py` and `new.py`. One deleted space inside an
invalid Python function is retained as one uncertain source change. This is
actual CLI output, not a generated screenshot or a VS Code GUI capture.

Wheel: `intentumdiff_python-0.0.2b1-py3-none-manylinux_2_39_x86_64.whl`
SHA256: `4568c16f389d9c030bc01b9f47cdc30df6e146b4210dc3d496b05cf069c0a97d`.
Built with `maturin build --bindings cffi --profile dev --no-default-features`
from these core source changes. Rust 1.95.0, Python 3.12; Linux development
correctness build, not a release artifact or platform certification.

Parser components restored from published main CI artifacts, ZIP SHA256 verified:

| Parser | Commit | Artifact | ZIP SHA256 |
|---|---|---|---|
| Python | `4d825be9e26f96871539b990d1171b1815b8538f` | 9063637474 | `e00bd8d4e509fe9d96868859e438f6efa65410dfc06bc4eaf23e1831a63fca4c` |
| JS/TS | `c7ae6476889dd0db4a88907735ed06c7f155d07b` | 9063587102 | `d31517777bfcd5e08f37492e4fbe3dafd8fc5c8738dd751a8be2157dc6275741` |

## Scope and remaining work

Native Python now degrades internally, and other parsers can use the shared core
fallback when their output retains error/missing markers. The JS/TS parser drops
errors for `function f(` → `function g(`, producing two empty semantic trees and
hiding an edit in both Rust and Python routes. Independent review confirmed this
separate gap: [JS/TS parser #4](https://github.com/buchochelliq-labs/intentumdiff-js-ts-parser/issues/4).
Neither implementation was treated as an oracle. Multi-language incomplete-code
readiness and an installed VSIX GUI walkthrough remain outstanding. No runtime
packaging choice, merge, tag or release is made here.
