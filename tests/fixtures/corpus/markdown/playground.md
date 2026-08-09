# Release Guide

Intro paragraph with the release cadence of 14 days.

## PrepareArtifacts

Build the wheels and verify the 42 pins.

```bash
maturin build --release
```

## PublishRelease

Tag the commit and push the artifacts.

## ArchiveNotes

Store the notes in the shared drive.
