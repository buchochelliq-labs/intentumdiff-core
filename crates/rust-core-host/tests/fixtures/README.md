# Callable parser shape regression fixtures

`callable_parser_shapes.json` records actual SemanticNode trees from published Wasm parsers for the eight rename regressions exposed by Python PR55's full CI matrix. Input sources come from the Python repository's existing `tests/fixtures/corpus/<language>/playground.*` files at candidate `95fe2381d618d58009999f804cfdec4ee3c28ed1`. Each new source replaces the first discovered callable name with its Renamed suffix, as the existing construct-edit matrix does.

The trees retain the real node IDs, types, labels, hashes, source positions and children. Only unrelated root siblings and optional facts/type metadata were removed. Full original source text remains so UTF-8 positions and opaque-node whole-file checks use real evidence. These are snapshots of parser output, not hand-built parameter-node assumptions.

| Parser | Source commit | GitHub artifact |
|---|---|---|
| assemblyscript | `9177b9314d48b7d0fc73b2dde3cc49d7f44e1c9c` | 9063519560 |
| bash | `3e743d8e556161697d4fb7bcc009eaaec5e83095` | 9063519039 |
| dart | `ada80502365bdacb15b898f382920c730e19806c` | 9063537583 |
| delphi | `74c7eb0863a85062f31a82c2714eb72a41002349` | 9063544318 |
| kotlin | `62699fabf203b9de08c525b596c2062f9d7d2f9c` | 9063586935 |
| powershell | `2d58f03f06b0c52e9038666d42191ee912a7ed09` | 9063628839 |
| swift | `a798176579e952be1f22fcbd29686fc8dd6fd86c` | 9063669418 |
| vue | `5fbe3e71d2a3ed21226cb0cbebe9c2e8033a89f7` | 9063686166 |

Artifact ZIP SHA256 digests were verified against GitHub's artifact metadata before extraction. The public Python construct-edit matrix also runs against these actual components. The Rust snapshot test covers all eight shapes without requiring downloads during unit tests. Do not regenerate to hide a regression; inspect every changed source/tree and preserve independent semantic expectations.
