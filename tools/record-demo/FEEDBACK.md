# rs-rich integration feedback — 0.0.2 hardening

Tested published crates: rs-rich 0.0.9 and rs-rich-record 0.0.3.

## What worked

- crates.io resolved both packages without access to the GitHub repository.
- `default-features = false` allowed bounded CLI styling without syntax/Markdown support.
- `Text` kept user-controlled error content literal, including `[red]` in a filename.
- Interactive prefix colour, redirected stderr, failure exit code and NO_COLOR were checked
  against real processes. No semantic engine changes were needed.
- The recorder ran a real installed wheel in a clean virtual environment through a PTY.
  PNG, SVG, text grids, asciinema, GIF, HTML and MP4 were produced. PNG was inspected;
  FFmpeg read the MP4. The sample tape ran successfully.
- A missing dependency caused the tape to fail with its line number and actual screen text.
  That diagnostic made the host setup problem clear; it was not a recorder defect.

## Integration friction and suggested improvements

These are API/documentation observations, not confirmed rendering defects.

1. **Newline ownership:** `render_to_string` omits the trailing newline. Our first stderr
   integration used `eprint!`, leaving the shell prompt on the error line. Fixed locally
   with `eprintln!`. A small documented stderr example would prevent this mistake.
2. **Style inheritance:** `Text::styled("error: ", "bold red")` followed by
   `append(message, None)` applies the base style to the whole message. We now start with
   `Text::new("")` and style only the prefix. Show this distinction in the short examples.
3. **Output destination:** the default console discovers stdout. For stderr we explicitly
   check stderr's terminal status and render to a string before writing there. A documented
   destination-aware console/writer recipe would help CLI adopters.
4. **Recorder entry point:** rs-rich-record is a library; the shell command is shipped by
   rs-rich-cli. The README states this, but a copyable standalone Cargo example makes the
   dependency route easier when only the recorder is wanted.
5. **Tape output selection:** callers must pass `recording.formats(Formats::ALL)` to
   `record::write` to honour `Output png` etc. Passing `Formats::ALL` directly generates all
   formats. We corrected our wrapper; a high-level write-selected helper/example would help.
6. **MP4 availability:** the API documents MP4 as conditional on FFmpeg. Release-evidence
   tooling should explicitly assert that the requested video exists, rather than equating
   a successful write with a produced MP4.

## Not yet established

Windows/ConPTY recording, macOS recording, screen-reader accessibility, long-running
captures, and comprehensive Unicode rendering have not been certified by these checks.
No rs-rich performance or cross-platform rendering claims are made from one Linux capture.
