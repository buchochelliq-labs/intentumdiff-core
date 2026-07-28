# Security policy

## Reporting

Report suspected vulnerabilities privately via GitHub security advisories on this repository
(Security → Report a vulnerability). Please do not open public issues for security reports.

## The engine's security model

- **Wasm plugin sandbox** — parser/renderer components run under wasmtime with fuel limits,
  a linear-memory cap, no filesystem/network/clock ambient capabilities, and bounded outputs.
  A misbehaving component traps; it cannot crash or escape the host.
- **Supply-chain controls** — registry refs are validated against traversal/injection
  patterns; strict mode requires full commit SHAs; plugin artifacts verify against SHA-256
  checksums pinned in [intentdiff-registry](https://github.com/buchochelliq-labs/intentdiff-registry);
  `dep_hashes` pin pip installs.
- **Subprocess hygiene** — VCS CLIs (git/hg/svn/p4) are invoked with argument-injection
  guards (no leading `-`, no NUL/newline, validated refs and depot paths).
- **No network, no telemetry** — the engine performs no network I/O; analytics are local.

Binding repos and the extension inherit this model and link here rather than restating it.
