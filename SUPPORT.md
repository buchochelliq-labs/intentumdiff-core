# Support & issue routing

IntentumDiff spans multiple repos — file where the problem lives:

| Symptom | Repo |
|---|---|
| Wrong/missing semantic classification, engine crash, CLI, cache, guardrail evaluation, image diff | **intentumdiff-core** (here) |
| One language parses wrong | `intentumdiff-<lang>-parser` |
| Plugin SDK / WIT contract / authoring a parser | `intentumdiff-plugin-sdk` |
| Plugin listing, checksums, trust tiers | `intentumdiff-registry` |
| `pip install intentumdiff`, the Python API, the Python CLI packaging | `intentumdiff-python` |
| VS Code experience (lenses, panel, decorations) | `intentumdiff-vscode` |
| Editor IPC / spawning problems | `intentumdiff-live-server` |
| LSP diagnostics/lenses over LSP | `intentumdiff-lsp` |
| Go / Java bindings | `intentumdiff-go` / `intentumdiff-java` |

Not sure? File here — we'll route it. Security reports: see [SECURITY.md](SECURITY.md)
(private advisories, never public issues).
