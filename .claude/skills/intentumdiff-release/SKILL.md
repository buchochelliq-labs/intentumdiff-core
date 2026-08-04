---
name: intentumdiff-release
description: How IntentumDiff ships a release — the release-candidate branch, what PRs target, and how a version reaches PyPI, the VS Code Marketplace and Open VSX. Use this whenever you open a PR, cut or tag a release, decide which branch to base work on, or answer "where does this change go?". Applies to EVERY repo in the estate (intentumdiff-core, -python, -vscode, -ast, -registry, -plugin-sdk and all 69 parser repos). Read it before creating a branch — targeting the wrong base is the most common and most expensive mistake here.
---

# Releasing IntentumDiff

## The rule

**Work is never merged straight to `main`.** Every release gets a **release-candidate
branch**, and PRs target *that*. `main` only moves when the RC is released.

```
feature branch  ──PR──▶  release/v0.0.2-rc  ──PR──▶  main  ──tag──▶  published
```

**The next release is `0.0.2`**, so today's RC branch is:

```
release/v0.0.2-rc
```

## Why

`main` is what gets tagged and published, and publishing is irreversible: **PyPI has no
delete**, a Marketplace version cannot be overwritten, and a burned version number is burned
for everyone. Merging directly to `main` means every merge is a potential release, so the
branch is only ever as good as the last thing pushed to it.

An RC branch makes the release a *decision* rather than a side effect. It also gives a place
to accumulate and verify a set of changes together — which matters here because the estate
is 80 repos with cross-repo dependencies, and a change that is green alone can still break
the combination.

## What to do

**Starting work**

```bash
git fetch origin
git checkout -b feat/my-change origin/release/v0.0.2-rc   # base on the RC, NOT main
```

Basing on `main` when the RC has moved ahead produces a PR carrying every RC commit. That
has happened before and needed a full rescope; `git checkout -B <branch> origin/<base>`
plus a cherry-pick is the recovery.

**Opening the PR**

```bash
gh pr create --base release/v0.0.2-rc --head feat/my-change
```

Branch protection requires a PR and passing checks; approvals are set to **0** because this
is a solo-maintainer estate, so a review requirement above zero would make every merge
impossible.

**Cutting the release**

1. RC is green and complete → PR `release/v0.0.2-rc` → `main`
2. Verify `main` is green **before** tagging — this is the step that has caught every real
   release bug so far
3. Tag from `main`; the tag triggers publication
4. Open the next RC branch immediately, so in-flight work has a base

## Version rules

- The tag **must** match the version in the manifest — `pyproject.toml`, `package.json`,
  `Cargo.toml`. Workflows assert this and fail closed, deliberately: a mismatch means the
  published artefact and the git history disagree about what shipped.
- **Derive the expected version from the tag**, never a literal. A hardcoded version goes
  stale silently and fails at the very end of a release, after every platform wheel has been
  built, blaming the artefact when the gate was wrong.
- A published version is never reused. If a release fails after publishing, the next attempt
  is a new version.

## Cross-repo releases

Repos depend on each other by **git tag**, not branch — `intentumdiff-plugin-sdk`,
`intentumdiff-ast`. A tag is immutable, so it keeps the vocabulary and ABI stable underneath
consumers.

The consequence, learned the hard way: **a rename or breaking change needs a NEW tag, and
every consumer re-pinned.** After the IntentumDiff rebrand the components still pinned SDK
`v0.1.0`, where the package was named `intentdiff-plugin-sdk`, so the build failed with
"no matching package named `intentumdiff-plugin-sdk`". The dependency key was renamed; the
pinned tag was not.

Order for a coordinated release:

1. Release the **leaf** first (`intentumdiff-ast`, `intentumdiff-plugin-sdk`)
2. Re-pin consumers to the new tag
3. Release the consumers

## What NOT to do

- **Do not merge to `main` directly.** Even as an admin — protection allows it, which is
  precisely why the discipline has to be deliberate.
- **Do not tag before CI on `main` is green.** Publishing is irreversible; a red suite is
  free to fix beforehand and impossible to fix afterwards.
- **Do not change the WIT package namespace** to match a rename. It is a wire contract, not
  a brand: all 69 certified components are pinned against `intentdiff:plugin@1.0.0`, and
  renaming it breaks every one of them.
- **Do not re-pin the registry casually.** Its checksums certify specific component builds.
  Re-pinning is a deliberate release task with its own verification, never a side effect.
