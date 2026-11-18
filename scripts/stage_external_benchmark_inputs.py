"""Stage benchmark components from registry-matched wheel bytes and locked CI archives.

The immutable registry is the authority for BOTH commit and SHA-256. A published
wheel is only a convenient byte mirror; its own manifest cannot authorize bytes.
Reviewed CI archives must match the registry and immutable artifact lock. Downloads
are explicit; ordinary staging uses already downloaded ZIPs without network I/O.
This is CI tooling, not part of the runtime engine.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
from pathlib import Path

import yaml

# Use exactly the same pinned-artifact and redirect-safe acquisition code as Tier-C CI.
_spec = importlib.util.spec_from_file_location("parser_provision", Path(__file__).with_name("provision_parser_components.py"))
provision = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(provision)


def _commit(value: str) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{40}", value) is not None


def checked_registry(path: Path, commit: str) -> dict:
    """Bind local registry input bytes to the specified immutable Git object."""
    if not _commit(commit):
        raise ValueError("immutable registry commit required")
    path = path.resolve()
    root = Path(subprocess.check_output(
        ["git", "-C", str(path.parent), "rev-parse", "--show-toplevel"], text=True).strip())
    relative = path.relative_to(root).as_posix()
    committed = subprocess.check_output(["git", "-C", str(root), "show", f"{commit}:{relative}"])
    if path.read_bytes() != committed:
        raise ValueError("registry bytes do not match the specified committed registry")
    return yaml.safe_load(committed)


def stage(registry: dict, inventory: dict, published_dir: Path, out: Path, *,
          registry_commit: str, artifact_lock: dict | None = None,
          artifacts_dir: Path | None = None) -> None:
    if not _commit(registry_commit):
        raise ValueError("immutable registry commit required")
    if inventory.get("schema_version") != 1 or not inventory.get("parsers"):
        raise ValueError("parser inventory schema_version 1 required")
    provenance = json.loads((published_dir / "wasm_provenance.json").read_text())
    pins = provision.registry_component_pins(registry)
    if (artifact_lock is None) != (artifacts_dir is None):
        raise ValueError("artifact lock and archive directory are both required")
    locked = provision.checked_artifact_lock(artifact_lock, pins, registry_commit) if artifact_lock else {}
    checked = []
    names = {entry["wasm"] for entry in inventory["parsers"].values()}
    if set(locked) - names:
        raise ValueError("locked component is outside the required inventory")
    for name in sorted(names):
        if not provision.flat_component(name):
            raise ValueError(f"invalid flat component name {name!r}")
        if name not in pins:
            raise ValueError(f"unregistered component {name}")
        pin = pins[name]
        digest, commit = pin["sha256"], pin["commit"]
        repository = "https://github.com/" + pin["repository"]
        if name in locked:
            artifact = locked[name]
            archive = artifacts_dir / f"{artifact['artifact_id']}.zip"
            try:
                payload = provision.verified_archive_payload(name, artifact, archive.read_bytes())
            except OSError as error:
                raise ValueError(f"missing archive for {name}: {error}") from error
            source = {"origin": "locked_ci_archive", "commit": commit, "repository": repository,
                      "artifact_id": artifact["artifact_id"], "artifact_archive_sha256": artifact["archive_sha256"],
                      "artifact_member": artifact["archive_member"]}
        else:
            record = provenance.get("artifacts", {}).get(name, {})
            original_source = record.get("source", {})
            if original_source.get("commit") != commit or record.get("sha256") != digest:
                raise ValueError(f"published source commit/checksum disagrees with registry for {name}")
            if original_source.get("repository") != repository:
                raise ValueError(f"published repository disagrees with registry for {name}")
            source = {"origin": "published_wheel", "commit": commit, "repository": repository}
            try:
                payload = (published_dir / name).read_bytes()
            except OSError as error:
                raise ValueError(f"missing component {name}: {error}") from error
        if hashlib.sha256(payload).hexdigest() != digest:
            raise ValueError(f"component checksum mismatch for {name}")
        checked.append((name, payload, {"component": name, "sha256": digest,
                                      "registry_commit": registry_commit, **source}))
    # No partial manifest can certify a failed validation. Output is a fresh dir.
    manifest = {**inventory, "components": [record for _, _, record in checked]}
    files = {name: payload for name, payload, _ in checked}
    files["parser_manifest.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    provision.publish_files(out, files)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--registry-commit", required=True)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--published-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--lock", type=Path, required=True)
    parser.add_argument("--artifacts-dir", type=Path, required=True)
    parser.add_argument("--download-artifacts", action="store_true",
                        help="download exact locked GitHub Actions archives before offline staging")
    args = parser.parse_args()
    try:
        registry = checked_registry(args.registry, args.registry_commit)
        lock = json.loads(args.lock.read_bytes())
        locked = provision.checked_artifact_lock(lock, provision.registry_component_pins(registry), args.registry_commit)
        if args.download_artifacts:
            token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
            if not token:
                raise ValueError("GH_TOKEN or GITHUB_TOKEN required for artifact acquisition")
            provision.download_locked_artifacts(locked, token, args.artifacts_dir)
        stage(registry, json.loads(args.inventory.read_text()), args.published_dir, args.out,
              registry_commit=args.registry_commit, artifact_lock=lock, artifacts_dir=args.artifacts_dir)
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"benchmark provisioning failed: {error}\n")


if __name__ == "__main__":
    main()
