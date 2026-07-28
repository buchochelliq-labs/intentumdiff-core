# Support & issue routing

IntentDiff spans multiple repos — file where the problem lives:

| Symptom | Repo |
|---|---|
| Wrong/missing semantic classification, engine crash, CLI, cache, guardrail evaluation, image diff | **intentdiff-core** (here) |
| One language parses wrong | `intentdiff-<lang>-parser` |
| Plugin SDK / WIT contract / authoring a parser | `intentdiff-plugin-sdk` |
| Plugin listing, checksums, trust tiers | `intentdiff-registry` |
| `pip install intentdiff`, the Python API, the Python CLI packaging | `intentdiff-python` |
| VS Code experience (lenses, panel, decorations) | `intentdiff-vscode` |
| Editor IPC / spawning problems | `intentdiff-live-server` |
| LSP diagnostics/lenses over LSP | `intentdiff-lsp` |
| Go / Java bindings | `intentdiff-go` / `intentdiff-java` |

Not sure? File here — we'll route it. Security reports: see [SECURITY.md](SECURITY.md)
(private advisories, never public issues).
