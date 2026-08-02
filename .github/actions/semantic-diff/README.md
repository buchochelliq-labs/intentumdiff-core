# IntentDiff PR Guard Action

Local pre-release GitHub Action for semantic diff checks, protected config
guardrails, SARIF upload, report artifacts, static HTML review output, and
optional PR comments.

This action expects the repository to be checked out first. Use
`fetch-depth: 0` so IntentDiff can compare the requested refs.

```yaml
name: Semantic diff

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write      # only needed when comment: true
  security-events: write    # only needed when upload-sarif: true

jobs:
  semantic-diff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683
        with:
          fetch-depth: 0
          persist-credentials: false

      - uses: ./.github/actions/semantic-diff
        with:
          strict: true
          comment: true
```

## Common Inputs

| Input | Default | Purpose |
|---|---|---|
| `base-ref` | PR base SHA or `HEAD~1` | Old ref. |
| `head-ref` | PR head SHA or `HEAD` | New ref. |
| `policy` | auto-discovered `intentdiff.yaml` | Guardrail policy override. |
| `strict` | `false` | Fail with exit code `2` for immutable guardrail violations. |
| `paths` | all changed files | Newline- or comma-separated glob filters. |
| `comment` | `false` | Post/update a sticky PR summary comment. |
| `upload-sarif` | `true` | Upload guardrail-only SARIF to code scanning. |
| `upload-artifact` | `true` | Upload JSON/SARIF/Markdown reports. |
| `fail-on-semantic-change` | `false` | Fail with exit code `3` for any semantic change. |

## Reports

The action writes these files under `report-dir` (`intentdiff-report` by
default):

- `semantic-diff.json`
- `guardrails.json`
- `guardrails.sarif`
- `summary.md`
- `intentdiff-review.html`

SARIF is guardrail-only in this slice. The JSON report contains the full
`CommitDiff`, including per-file `SemanticDiff` output and cross-file changes.
The HTML report is a static, no-script artifact intended for download from the
workflow run; it approximates the richer review surface without needing a hosted
GitHub App.

## Status

This action installs the **published `intentdiff` wheel** from PyPI (self-contained: engine +
parser components) and runs the PR guard shipped alongside it (`github_action.py`), against the
consumer's checkout. It therefore activates once the package is published; pin a version with
the `intentdiff-version` input for reproducible runs.

```yaml
- uses: buchochelliq-labs/intentdiff-core/.github/actions/semantic-diff@main
  with:
    comment: true
    intentdiff-version: "0.1.0"
```
