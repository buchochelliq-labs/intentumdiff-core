"""Stage the parser components the Tier-C certification tests load.

The engine's 25 Tier-C tests (`tier-c-wasm`, default ON) instantiate real parser
components out of a staging dir. In the monorepo that dir is checked out beside the
crate; in this extracted repo the components live in the sibling
`intentumdiff-<lang>-parser` repos, which publish each build as a `parser-wasm` artifact.

This script pulls the components named on the command line from the immutable registry's
pinned successful CI builds and stages them under --out, so the workflow can point
`INTENTUMDIFF_TEST_WASM_DIR` at it and run the FULL gate instead of
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
import re
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
ORG = "buchochelliq-labs"
API = "https://api.github.com"

# The components the engine's own tests name (crates/rust-core-host/src/tests_*.rs).
# Keys are the parser slug: repo = intentumdiff-<slug>-parser, file = <slug_>parser.wasm.
#
# `python`, `go`, `js-ts` are named by the Tier-C certification tests.
# `ini` and `asm` are named by the language-parity harness (#23) as its wasm-backed
# increment languages - `python` runs the certified NATIVE batch and `generic`
# short-circuits before any component, so without these two the harness would exercise
# no Wasm dispatch at all.
#
# Keep this list in step with the harness's INCREMENT_LANGUAGES. It is not cosmetic: the
# harness FAILS when a mapped component is absent, deliberately, because the alternative
# is a language silently dropping out of the matrix while the suite still reports green.
# JSON is also required by the native parser fuel telemetry regressions.
# Source-judged Core #192 tests require actual parser bytes, never synthetic CSTs.
# Tier-C certification and source regressions require actual verified components.
# Keep the shared correctness repair batch enabled on every independent fix PR.
TIER_C_COMPONENTS = [
    "python", "go", "js-ts", "ini", "asm", "json", "clojure", "assemblyscript",
    "css", "scss", "yaml", "make", "proto", "puppet", "toml",
    # 0.0.3 source-judged regressions (#247 XML/HTML whitespace, #271 Elixir visibility,
    # #274/#275 Markdown code blocks and ATX headings, MDX headings).
    "elixir", "html", "xml", "markdown", "mdx",
]


class _StripAuthOnRedirect(urllib.request.HTTPRedirectHandler):
    """Drop the Authorization header when a redirect leaves the API host.

    `artifacts/<id>/zip` 302s to Azure blob storage, which signs the request in the URL
    and REJECTS a bearer token it did not issue (401 locally, 403 on a runner). urllib
    re-sends every header across a redirect by default, so the naive fetch fails for a
    reason that reads like a permissions problem and isn't one.
    """

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != "https":
            raise urllib.error.HTTPError(newurl, 403, "refusing non-HTTPS artifact redirect", headers, fp)
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
            "User-Agent": "intentumdiff-core-provision",
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
    query = "status=success&per_page=100"
    if ref:
        query += f"&head_sha={ref}"
    result: list[dict] = []
    # GitHub caps filtered workflow-run searches at 1,000 results. Walk the
    # available history instead of letting scheduled jobs hide the pinned build.
    for page in range(1, 11):
        try:
            runs = json.loads(_get(
                f"{API}/repos/{ORG}/{repo}/actions/runs?{query}&page={page}", token))
        except urllib.error.HTTPError as exc:
            raise RuntimeError(
                f"{repo}: HTTP {exc.code} listing workflow runs - the token needs the "
                f"Actions: Read permission on the parser repos"
            ) from exc
        batch = runs.get("workflow_runs") or []
        result.extend(run for run in batch
                      if (not ref or run.get("head_sha") == ref)
                      and not run.get("path", "").startswith("dynamic/"))
        if len(batch) < 100:
            break
    return result


def fetch_component(slug: str, token: str, ref: str | None = None) -> tuple[str, str, str, bytes]:
    """Fetch one parser component. Returns (filename, source run sha, sha256, payload).

    Deliberately does NOT write: nothing unverified should reach the staging dir, or a
    later step could pick up a component the registry never vouched for.
    """
    repo = f"intentumdiff-{slug}-parser"
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
                # The parser crates build `intentumdiff_<slug>_parser.wasm`; the host
                # stages the component under its unprefixed plugin name. BOTH prefixes
                # must be accepted: the registry pins immutable artifacts BY REF, and the
                # ones pinned today were built before the IntentDiff -> IntentumDiff
                # rename, so they are still named intentdiff_*.wasm. Accepting only the
                # new prefix leaves the name prefixed and nothing ever matches `wanted`.
                name = Path(member).name
                for _prefix in ("intentumdiff_", "intentdiff_"):
                    if name.startswith(_prefix):
                        name = name[len(_prefix):]
                        break
                if name != wanted:
                    continue
                payload = archive.read(member)
                return name, run["head_sha"], hashlib.sha256(payload).hexdigest(), payload
        problems.append(f"run {run['id']} artifact contains no {wanted}")

    detail = "; ".join(problems) if problems else "no candidate runs"
    at = f" at pinned ref {ref[:8]}" if ref else ""
    raise RuntimeError(f"{repo}: could not obtain {wanted}{at} - {detail}")


# ── Registry pinning (#95) ────────────────────────────────────────────────────
# intentumdiff-registry is the root of trust: for each official plugin it pins BOTH the
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

REGISTRY_REPO = "intentumdiff-registry"
# Same vetted component inventory as the Python/native release candidate.
INPUTS_LOCK = REPO_ROOT / "scripts/external_benchmark_inputs.lock.json"
_INPUTS = json.loads(INPUTS_LOCK.read_bytes())
REGISTRY_REF = _INPUTS["registry_commit"]
LOCKED_COMPONENTS = _INPUTS["components"]


def _hex(value, size: int) -> bool:
    return isinstance(value, str) and re.fullmatch(rf"[a-f0-9]{{{size}}}", value) is not None


def flat_component(name) -> bool:
    return isinstance(name, str) and re.fullmatch(r"[a-z][a-z0-9_]*\.wasm", name) is not None


def registry_component_pins(document: dict) -> dict:
    """Registry alone authorizes each component's repository, source ref and bytes."""
    if not isinstance(document, dict) or not isinstance(document.get("plugins"), dict):
        raise ValueError("registry plugins mapping required")
    result = {}
    for plugin, entry in document.get("plugins", {}).items():
        if not isinstance(entry, dict) or not isinstance(entry.get("wasm_checksums", {}), dict):
            raise ValueError("invalid registry plugin entry")
        for name, digest in entry.get("wasm_checksums", {}).items():
            if (not flat_component(name) or name in result or not _hex(entry.get("ref"), 40)
                    or not _hex(digest, 64)
                    or not isinstance(plugin, str)
                    or re.fullmatch(r"intentumdiff-[a-z0-9-]+", plugin) is None):
                raise ValueError(f"invalid or duplicate registry component identity: {name}")
            result[name] = {"sha256": digest, "commit": entry["ref"], "repository": f"{ORG}/{plugin}"}
    if not result:
        raise ValueError("registry carries no component pins")
    return result


def checked_artifact_pin(name: str, pin: dict, registry_pin: dict | None = None) -> dict:
    if (not flat_component(name) or not isinstance(pin, dict)
            or type(pin.get("artifact_id")) is not int or pin["artifact_id"] <= 0
            or not _hex(pin.get("commit"), 40) or not _hex(pin.get("archive_sha256"), 64)
            or not _hex(pin.get("component_sha256"), 64)
            or not isinstance(pin.get("repository"), str)
            or re.fullmatch(rf"{ORG}/intentumdiff-[a-z0-9-]+", pin["repository"]) is None
            or pin.get("archive_member") not in (name, "intentumdiff_" + name, "intentdiff_" + name)):
        raise ValueError(f"invalid immutable artifact identity for {name}")
    if registry_pin is not None and any((
        pin["commit"] != registry_pin["commit"],
        pin["component_sha256"] != registry_pin["sha256"],
        pin["repository"] != registry_pin["repository"],
    )):
        raise ValueError(f"artifact source commit/checksum/repository disagrees with registry for {name}")
    return pin


def checked_artifact_lock(lock: dict, pins: dict, registry_commit: str) -> dict:
    if (not isinstance(lock, dict) or type(lock.get("schema_version")) is not int or lock["schema_version"] != 2
            or not _hex(registry_commit, 40) or lock.get("registry_commit") != registry_commit
            or not isinstance(lock.get("components"), dict) or not lock["components"]):
        raise ValueError("artifact lock requires schema 2 and the exact immutable registry commit")
    result = {}
    for name, pin in lock["components"].items():
        if name not in pins:
            raise ValueError(f"unregistered locked component {name}")
        result[name] = checked_artifact_pin(name, pin, pins[name])
    if len({p["artifact_id"] for p in result.values()}) != len(result):
        raise ValueError("artifact IDs must be unique")
    return result


def verified_archive_payload(name: str, pin: dict, blob: bytes) -> bytes:
    checked_artifact_pin(name, pin)
    if hashlib.sha256(blob).hexdigest() != pin["archive_sha256"]:
        raise ValueError(f"archive checksum mismatch for {name}")
    try:
        with zipfile.ZipFile(io.BytesIO(blob)) as archive:
            members = archive.infolist()
            if (len(members) != 1 or members[0].filename != pin["archive_member"]
                    or members[0].is_dir() or (members[0].external_attr >> 16) & 0o170000 == 0o120000):
                raise ValueError(f"archive must contain exactly one flat regular component for {name}")
            payload = archive.read(members[0])
    except (zipfile.BadZipFile, RuntimeError, OSError) as error:
        raise ValueError(f"invalid archive for {name}: {error}") from error
    if hashlib.sha256(payload).hexdigest() != pin["component_sha256"]:
        raise ValueError(f"component checksum mismatch for {name}")
    return payload


def fetch_locked_artifact(name: str, pin: dict, token: str) -> bytes:
    """Fetch one exact reviewed CI artifact, never a latest-run replacement."""
    checked_artifact_pin(name, pin)
    base = f"{API}/repos/{pin['repository']}/actions"
    artifact_url = f"{base}/artifacts/{pin['artifact_id']}"
    artifact = json.loads(_get(artifact_url, token))
    if not isinstance(artifact, dict) or not isinstance(artifact.get("workflow_run"), dict):
        raise ValueError(f"invalid GitHub artifact metadata for {name}")
    run = artifact["workflow_run"]
    if (type(artifact.get("id")) is not int or artifact["id"] != pin["artifact_id"]
            or artifact.get("name") != "parser-wasm" or artifact.get("expired") is not False
            or run.get("head_sha") != pin["commit"] or type(run.get("id")) is not int
            or run["id"] <= 0
            or (artifact.get("digest") is not None and artifact["digest"] != "sha256:" + pin["archive_sha256"])
            or (run.get("repository_id") is not None and run.get("head_repository_id") != run["repository_id"])):
        raise ValueError(f"GitHub artifact identity disagrees with locked input for {name}")
    workflow = json.loads(_get(f"{base}/runs/{run['id']}", token))
    if (not isinstance(workflow, dict) or type(workflow.get("id")) is not int
            or workflow["id"] != run["id"] or workflow.get("head_sha") != pin["commit"]
            or workflow.get("status") != "completed" or workflow.get("conclusion") != "success"):
        raise ValueError(f"artifact workflow did not succeed at the registry-pinned commit for {name}")
    blob = _get(artifact_url + "/zip", token)
    verified_archive_payload(name, pin, blob)
    return blob


def require_fresh_output(out: Path) -> None:
    if out.is_symlink() or (out.exists() and (not out.is_dir() or any(out.iterdir()))):
        raise ValueError("output directory must be empty; refusing stale component inputs")


def publish_files(out: Path, files: dict[str, bytes]) -> None:
    """Publish a fresh complete directory atomically, retaining no partial manifest."""
    require_fresh_output(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=f".{out.name}-", dir=out.parent) as temporary:
        root = Path(temporary)
        for name, payload in files.items():
            (root / name).write_bytes(payload)
        os.replace(root, out)


def download_locked_artifacts(locked: dict, token: str, out: Path) -> None:
    require_fresh_output(out)
    archives = {f"{pin['artifact_id']}.zip": fetch_locked_artifact(name, pin, token)
                for name, pin in sorted(locked.items())}
    publish_files(out, archives)


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
        f"{API}/repos/{ORG}/{REGISTRY_REPO}/contents/registry.yaml?ref={REGISTRY_REF}",
        token,
        accept="application/vnd.github.raw",
    )
    document = yaml.safe_load(raw.decode("utf-8"))
    verified = registry_component_pins(document)
    pins = {name: pin["sha256"] for name, pin in verified.items()}
    refs = {pin["repository"].split("/")[1]: pin["commit"] for pin in verified.values()}
    return pins, refs


def fetch_mirrored_component(
    slug: str, mirror: Path, ref: str | None, pinned: str | None,
) -> tuple[str, str, str, bytes] | None:
    """Use published bytes only when both independent registry pins match.

    CI artifacts expire. A wheel can mirror the exact old build without changing
    the registry's identity. An older wheel simply does not mirror a newer pin;
    corrupted bytes claiming the requested identity are an error, never a bypass.
    """
    name = f"{slug.replace('-', '_')}_parser.wasm"
    if Path(name).name != name or not ref or not pinned:
        raise RuntimeError(f"invalid mirrored component identity: {slug}")
    manifest = mirror / "wasm_provenance.json"
    if not manifest.is_file() or not (mirror / name).is_file():
        return None
    try:
        record = json.loads(manifest.read_text())["artifacts"].get(name)
        if not isinstance(record, dict) or record.get("source", {}).get("commit") != ref:
            return None
        if record.get("sha256") != pinned or record["source"].get("repository") != f"https://github.com/{ORG}/intentumdiff-{slug}-parser":
            raise RuntimeError(f"mirror source/checksum disagrees with registry for {name}")
        payload = (mirror / name).read_bytes()
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as exc:
        raise RuntimeError(f"invalid mirror provenance for {name}: {exc}") from exc
    digest = hashlib.sha256(payload).hexdigest()
    if digest != pinned:
        raise RuntimeError(f"mirror checksum mismatch for {name}")
    return name, ref, digest, payload


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default=str(REPO_ROOT / "build" / "wasm"),
                        help="directory to stage components into (INTENTUMDIFF_TEST_WASM_DIR)")
    parser.add_argument("--components", nargs="+", default=TIER_C_COMPONENTS,
                        help="parser slugs to stage (default: the Tier-C set)")
    parser.add_argument("--no-verify", action="store_true",
                        help="skip verification against the registry's pinned checksums "
                             "(for bringing up a component the registry does not pin yet)")
    parser.add_argument("--mirror-dir", type=Path,
                        help="published wheel Wasm directory; verify exact registry ref and SHA-256 before use")
    args = parser.parse_args()
    if args.no_verify and args.mirror_dir:
        parser.error("--mirror-dir requires registry verification")

    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token:
        sys.exit("need GH_TOKEN or GITHUB_TOKEN with read access to the parser repos")

    out = Path(args.out)
    if out.exists() and (not out.is_dir() or any(out.iterdir())):
        parser.error("output directory must be empty; refusing stale component inputs")
    if len(set(args.components)) != len(args.components):
        parser.error("components must be unique")
    if args.no_verify:
        pins, refs = {}, {}
        print("WARNING: --no-verify - components are NOT checked against the registry")
    else:
        pins, refs = load_registry_pins(token)

    failures = []; checked = {}; records = []
    for slug in args.components:
        name = f"{slug.replace('-', '_')}_parser.wasm"
        if not flat_component(name):
            failures.append(f"invalid component slug: {slug}")
            continue
        ref = refs.get(f"intentumdiff-{slug}-parser")
        pinned = pins.get(name)
        try:
            locked = LOCKED_COMPONENTS.get(name) if not args.no_verify else None
            if locked is not None:
                checked_artifact_pin(name, locked, {"commit": ref, "sha256": pinned,
                    "repository": f"{ORG}/intentumdiff-{slug}-parser"})
                payload = verified_archive_payload(name, locked, fetch_locked_artifact(name, locked, token))
                head_sha, digest = locked["commit"], locked["component_sha256"]
                source = {"origin": "locked_ci_archive", "artifact_id": locked["artifact_id"],
                          "artifact_archive_sha256": locked["archive_sha256"]}
            else:
                mirrored = fetch_mirrored_component(slug, args.mirror_dir, ref, pinned) if args.mirror_dir else None
                actual_name, head_sha, digest, payload = mirrored or fetch_component(slug, token, ref)
                if actual_name != name:
                    raise ValueError(f"unexpected component name: {actual_name}")
                source = {"origin": "unverified_ci" if args.no_verify else
                          "published_wheel" if mirrored else "registry_pinned_ci"}
            if not args.no_verify and (not ref or head_sha != ref or not pinned or digest != pinned
                                       or hashlib.sha256(payload).hexdigest() != pinned):
                raise ValueError(f"source commit/checksum disagrees with registry for {name}")
            checked[name] = payload
            records.append({"component": name, "commit": head_sha, "sha256": digest,
                            "repository": f"https://github.com/{ORG}/intentumdiff-{slug}-parser", **source})
        except (RuntimeError, ValueError, OSError, urllib.error.HTTPError) as exc:
            failures.append(f"{slug}: {exc}")
    if failures:
        print(f"FAILED to stage {len(failures)} component(s):", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        sys.exit(1)
    manifest = {"schema_version": 1, "registry_commit": None if args.no_verify else REGISTRY_REF,
                "verified": not args.no_verify, "components": records}
    checked["parser_component_provenance.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    publish_files(out, checked)
    print(f"{len(records)} component(s) staged in {out}")


if __name__ == "__main__":
    main()
