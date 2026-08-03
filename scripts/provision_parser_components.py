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


def _get(url: str, token: str, accept: str = "application/vnd.github+json") -> bytes:
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": accept,
            "User-Agent": "intentdiff-core-provision",
        },
    )
    with _OPENER.open(req, timeout=120) as resp:
        return resp.read()


def _successful_runs(repo: str, token: str, ref: str | None) -> list[dict]:
    """Successful runs to take a component from, newest first.

    With a pinned ref, ask only for runs at that commit. The registry pins BOTH a ref
    and a checksum, so "latest successful run" quietly ignores half the pin: any
    unrelated commit on a parser repo — a CI tweak, a Dependabot config — repoints this
    at a different build, and the checksum gate then fails for the wrong reason. It
    reads as "the component changed" when nothing about the component changed at all.
    """
    if ref:
        query = f"head_sha={ref}&status=success&per_page=20"
    else:
        # No pin (a component the registry does not vouch for yet, or --no-verify).
        query = "status=success&per_page=1"

    # `list-runs` is the call that needs Actions: Read — a token with only
    # contents/metadata read (enough for the private git deps) 403s here, so name the
    # stage in the error rather than leaving a bare "HTTP 403".
    try:
        runs = json.loads(_get(f"{API}/repos/{ORG}/{repo}/actions/runs?{query}", token))
    except urllib.error.HTTPError as exc:
        raise RuntimeError(
            f"{repo}: HTTP {exc.code} listing workflow runs - the token needs the "
            f"Actions: Read permission on the parser repos"
        ) from exc
    return runs.get("workflow_runs") or []


def fetch_component(slug: str, token: str, ref: str | None = None) -> tuple[str, str, str, bytes]:
    """Fetch one parser component. Returns (filename, source run sha, sha256, payload).

    Deliberately does NOT write: nothing unverified should reach the staging dir, or a
    later step could pick up a component the registry never vouched for.
    """
    repo = f"intentdiff-{slug}-parser"
    wanted = f"{slug.replace('-', '_')}_parser.wasm"

    workflow_runs = _successful_runs(repo, token, ref)
    if not workflow_runs:
        if ref:
            raise RuntimeError(
                f"{repo}: no successful workflow run at the registry-pinned ref "
                f"{ref[:8]}. Either that run's history was removed, or the registry "
                f"pins a commit whose CI never went green - fix the pin via a registry "
                f"PR rather than falling back to a different build."
            )
        raise RuntimeError(f"{repo}: no successful workflow run to take a component from")

    # Several workflows can run on one commit (CI, CodeQL, Dependabot); only one
    # publishes parser-wasm. Walk them rather than assuming the newest is the right one.
    problems: list[str] = []
    for run in workflow_runs:
        artifacts = json.loads(_get(run["artifacts_url"], token))
        artifact = next(
            (a for a in artifacts.get("artifacts", []) if a["name"] == "parser-wasm"), None)
        if artifact is None:
            problems.append(f"run {run['id']} published no 'parser-wasm' artifact")
            continue
        if artifact.get("expired"):
            problems.append(f"the 'parser-wasm' artifact of run {run['id']} has expired")
            continue

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
                return name, run["head_sha"], hashlib.sha256(payload).hexdigest(), payload
        problems.append(f"run {run['id']} artifact contains no {wanted}")

    detail = "; ".join(problems) if problems else "no candidate runs"
    at = f" at pinned ref {ref[:8]}" if ref else ""
    raise RuntimeError(f"{repo}: could not obtain {wanted}{at} - {detail}")


# ── Registry pinning (#95) ────────────────────────────────────────────────────
# intentdiff-registry is the root of trust: for each official plugin it pins BOTH the
# commit (`ref`) and the component's SHA-256. Provisioning honours both — it asks for
# the successful run at the pinned ref, then verifies the bytes against the pinned
# checksum. Together those make the flow a supply-chain control rather than a download.
#
# Using the ref matters as much as the checksum. Taking "the latest successful run"
# instead means any unrelated commit on a parser repo silently repoints provisioning at
# a different build, and the checksum gate then fires for the wrong reason — reporting
# "the component changed" when only the commit did.
#
# Component builds are reproducible (two independent CI runs of one commit produce a
# byte-identical .wasm), so a mismatch AT THE PINNED REF means the component genuinely
# changed, and the fix is a registry PR through the vet gate — not a bypass here.

REGISTRY_REPO = "intentdiff-registry"


def load_registry_pins(token: str) -> tuple[dict[str, str], dict[str, str]]:
    """Fetch registry.yaml.

    Returns ({component filename: sha256}, {plugin repo name: pinned ref}). The ref half
    used to be dropped on the floor, which is what let provisioning drift onto whatever
    the parser repo happened to build last.
    """
    try:
        import yaml
    except ImportError:  # pragma: no cover - environment problem, not logic
        sys.exit("registry verification needs PyYAML (pip install pyyaml), or pass --no-verify")

    raw = _get(
        f"{API}/repos/{ORG}/{REGISTRY_REPO}/contents/registry.yaml",
        token,
        accept="application/vnd.github.raw",
    )
    document = yaml.safe_load(raw.decode("utf-8"))
    pins: dict[str, str] = {}
    refs: dict[str, str] = {}
    for plugin, entry in (document.get("plugins") or {}).items():
        pins.update(entry.get("wasm_checksums") or {})
        if entry.get("ref"):
            refs[plugin] = entry["ref"]
    if not pins:
        sys.exit("registry.yaml carries no wasm_checksums - refusing to 'verify' nothing")
    return pins, refs


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default=str(REPO_ROOT / "build" / "wasm"),
                        help="directory to stage components into (INTENTDIFF_TEST_WASM_DIR)")
    parser.add_argument("--components", nargs="+", default=TIER_C_COMPONENTS,
                        help="parser slugs to stage (default: the Tier-C set)")
    parser.add_argument("--no-verify", action="store_true",
                        help="skip verification against the registry's pinned checksums "
                             "(for bringing up a component the registry does not pin yet)")
    args = parser.parse_args()

    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token:
        sys.exit("need GH_TOKEN or GITHUB_TOKEN with read access to the parser repos")

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    if args.no_verify:
        pins, refs = {}, {}
        print("WARNING: --no-verify - components are NOT checked against the registry")
    else:
        pins, refs = load_registry_pins(token)

    failures = []
    for slug in args.components:
        # Take the build the registry pins, not whatever ran most recently.
        ref = refs.get(f"intentdiff-{slug}-parser")
        try:
            name, head_sha, digest, payload = fetch_component(slug, token, ref)
        except (RuntimeError, urllib.error.HTTPError) as exc:
            failures.append(f"{slug}: {exc}")
            continue

        if args.no_verify:
            (out / name).write_bytes(payload)
            print(f"staged {name}  from {head_sha[:8]}  sha256={digest}  (unverified)")
            continue

        pinned = pins.get(name)
        if pinned is None:
            failures.append(f"{slug}: {name} is not pinned in {REGISTRY_REPO}/registry.yaml")
        elif pinned != digest:
            failures.append(
                f"{slug}: {name} CHECKSUM MISMATCH - registry pins {pinned}, the artifact "
                f"of {head_sha[:8]} is {digest}. This is the build at the registry's own "
                f"pinned ref, so the component genuinely changed: re-pin it via a "
                f"registry PR through the vet gate."
            )
        else:
            (out / name).write_bytes(payload)
            print(f"staged {name}  from {head_sha[:8]}  sha256={digest}  (registry-verified)")

    if failures:
        print(f"\nFAILED to stage {len(failures)} component(s):", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        sys.exit(1)

    print(f"\n{len(args.components)} component(s) staged in {out}")


if __name__ == "__main__":
    main()
