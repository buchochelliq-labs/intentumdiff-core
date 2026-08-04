# Perceptual asset diff

Non-text assets get more than "binary changed": the engine decodes, compares, and explains
*what* changed, *where*, and whether it looks meaningful.

- **Engine-owned end to end**: image decoding, comparison metrics, artifact rendering
  (side-by-side, onion, swipe, difference, change lasso, hotspots, histograms), and git asset
  discovery all happen in this repo. Consumers (the CLI, the VS Code extension) only render
  the structured JSON + generated artifacts — no image processing outside the engine.
- **Supported formats**: PNG, JPG/JPEG, WEBP. Audio/video are documented extension points.

## CLI

```bash
intentumdiff assets diff --before old.png --after new.png --out .intentumdiff/assets --json
intentumdiff assets git  --base main --head HEAD          --out .intentumdiff/assets --json
```

The C ABI exposes the same operations (`diff_asset_image`, `diff_git_assets`) for bindings.
