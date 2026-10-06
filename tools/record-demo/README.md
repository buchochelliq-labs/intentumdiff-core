# Recording IntentumDiff demonstrations

This development-only executable uses the published `rs-rich-record =0.0.3`
crate. It is separate from the engine and shipping CLI dependencies.

```sh
cargo run --locked --manifest-path tools/record-demo/Cargo.toml --   tools/record-demo/version.tape path/to/output /absolute/path/to/installed/bin
```

The third argument selects the actual CLI under test through the recorder's
PATH prefix. For Python release evidence, use the bin directory of a clean
virtual environment containing the exact candidate wheel. For native evidence,
use the built native CLI directory. Do not substitute a mocked command.

The recorder captures a real PTY and writes PNG/SVG stills, text grids,
asciinema, GIF, HTML and MP4 (requires FFmpeg). Inspect the rendered media and
text grids before publishing. A successful capture is not correctness proof:
compare actual results with independently judged source expectations.

Retain the tape, sources, expected and actual results, wheel/native SHA-256,
Python and core commits, component manifest, platform, recorder version and
Cargo.lock alongside every published scenario. Published crate provenance is
its version and registry checksum; record the crate's VCS SHA when supplied.
The published recorder package records VCS commit
`61d031aee455572d9cd6cfa44febe4f3bd99b22a` in `.cargo_vcs_info.json`.
No GitHub checkout of rs-rich is required.
