"""Stage the parser components the Tier-C certification tests load.

The engine's 25 Tier-C tests (`tier-c-wasm`, default ON) instantiate real parser
components out of a staging dir. In the monorepo that dir is checked out beside the
crate; in this extracted repo the components live in the sibling
`intentdiff-<lang>-parser` repos, which publish each build as a `parser-wasm` artifact.

This script pulls the components named on the command line from those repos' latest
successful CI runs and stages them under --out, so the workflow can point
`INTENTDIFF_TEST_WASM_DIR` at it and run the FULL gate instead of
`cargo test --no-default-features`.

It is fail-closed: any component that cannot be staged is an error, because a silently
missing component turns a certification test into a confusing panic (or, worse, the
whole tier back into a skip).

Usage:
  GH_TOKEN=<org-scoped token> python scripts/provision_parser_components.py --out build/wasm
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
ORG = "buchochelliq-labs"
API = "https://api.github.com"

# The components the Tier-C tests name (crates/rust-core-host/src/tests_*.rs).
# Keys are the parser slug: repo = intentdiff-<slug>-parser, file = <slug_>parser.wasm.
TIER_C_COMPONENTS = ["python", "go", "js-ts"]


class _StripAuthOnRedirect(urllib.request.HTTPRedirectHandler):
    """Drop the Authorization header when a redirect leaves the API host.

    `artifacts/<id>/zip` 302s to Azure blob storage, which signs the request in the URL
    and REJECTS a bearer token it did not issue (401 locally, 403 on a runner). urllib
    re-sends every header across a redirect by default, so the naive fetch fails for a
    reason that reads like a permissions problem and isn't one.
    """

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        new = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new is not None and (
            urllib.parse.urlsplit(newurl).netloc != urllib.parse.urlsplit(req.full_url).netloc
        ):
            new.remove_header("Authorization")
        return new


_OPENER = urllib.request.build_opener(_StripAuthOnRedirect)


def _get(url: str, token: str) -> bytes:
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "User-Agent": "intentdiff-core-provision",
        },
    )
    with _OPENER.open(req, timeout=120) as resp:
        return resp.read()


def stage_component(slug: str, out: Path, token: str) -> tuple[str, str, str]:
    """Stage one parser component. Returns (filename, source run sha, sha256)."""
    repo = f"intentdiff-{slug}-parser"
    wanted = f"{slug.replace('-', '_')}_parser.wasm"

    # `list-runs` is the call that needs Actions: Read — a token with only
    # contents/metadata read (enough for the private git deps) 403s here, so name the
    # stage in the error rather than leaving a bare "HTTP 403".
    try:
        runs = json.loads(
            _get(f"{API}/repos/{ORG}/{repo}/actions/runs?status=success&per_page=1", token))
    except urllib.error.HTTPError as exc:
        raise RuntimeError(
            f"{repo}: HTTP {exc.code} listing workflow runs - the token needs the "
            f"Actions: Read permission on the parser repos"
        ) from exc
    workflow_runs = runs.get("workflow_runs") or []
    if not workflow_runs:
        raise RuntimeError(f"{repo}: no successful workflow run to take a component from")
    run = workflow_runs[0]

    artifacts = json.loads(_get(run["artifacts_url"], token))
    artifact = next((a for a in artifacts.get("artifacts", []) if a["name"] == "parser-wasm"), None)
    if artifact is None:
        raise RuntimeError(f"{repo}: run {run['id']} published no 'parser-wasm' artifact")
    if artifact.get("expired"):
        raise RuntimeError(f"{repo}: the 'parser-wasm' artifact of run {run['id']} has expired")

    blob = _get(artifact["archive_download_url"], token)
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        for member in archive.namelist():
            if not member.endswith(".wasm"):
                continue
            # the parser crates build `intentdiff_<slug>_parser.wasm`; the host stages
            # the component under its unprefixed plugin name.
            name = Path(member).name
            name = name[len("intentdiff_"):] if name.startswith("intentdiff_") else name
            if name != wanted:
                continue
            payload = archive.read(member)
            (out / name).write_bytes(payload)
            return name, run["head_sha"], hashlib.sha256(payload).hexdigest()

    raise RuntimeError(f"{repo}: run {run['id']} artifact contains no {wanted}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default=str(REPO_ROOT / "build" / "wasm"),
                        help="directory to stage components into (INTENTDIFF_TEST_WASM_DIR)")
    parser.add_argument("--components", nargs="+", default=TIER_C_COMPONENTS,
                        help="parser slugs to stage (default: the Tier-C set)")
    args = parser.parse_args()

    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token:
        sys.exit("need GH_TOKEN or GITHUB_TOKEN with read access to the parser repos")

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    failures = []
    for slug in args.components:
        try:
            name, head_sha, digest = stage_component(slug, out, token)
            print(f"staged {name}  from {head_sha[:8]}  sha256={digest}")
        except (RuntimeError, urllib.error.HTTPError) as exc:
            failures.append(f"{slug}: {exc}")

    if failures:
        print(f"\nFAILED to stage {len(failures)} component(s):", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        sys.exit(1)

    print(f"\n{len(args.components)} component(s) staged in {out}")


if __name__ == "__main__":
    main()
