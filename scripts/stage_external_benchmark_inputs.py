"""Stage benchmark components from a wheel and an optional reviewed C/C++ build.

The immutable registry is the authority for BOTH commit and SHA-256. A published
wheel is only a convenient byte mirror; its own manifest cannot authorize bytes.
Candidate C/C++ bytes must already have a matching proposed registry entry.
This tool does not fetch anything and is not part of the runtime engine.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

import yaml


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
          registry_commit: str, candidate_dir: Path | None = None,
          candidate_commit: str | None = None) -> None:
    if not _commit(registry_commit):
        raise ValueError("immutable registry commit required")
    if inventory.get("schema_version") != 1 or not inventory.get("parsers"):
        raise ValueError("parser inventory schema_version 1 required")
    provenance = json.loads((published_dir / "wasm_provenance.json").read_text())
    pins = {}
    for plugin, entry in registry.get("plugins", {}).items():
        for name, digest in entry.get("wasm_checksums", {}).items():
            if name in pins:
                raise ValueError(f"duplicate registry component {name}")
            if not _commit(entry.get("ref")) or not re.fullmatch(r"[a-f0-9]{64}", digest):
                raise ValueError(f"invalid registry identity for {name}")
            pins[name] = (digest, entry["ref"], plugin)
    checked = []
    names = {entry["wasm"] for entry in inventory["parsers"].values()}
    for name in sorted(names):
        if Path(name).name != name or "/" in name or "\\" in name or not name.endswith(".wasm"):
            raise ValueError(f"invalid flat component name {name!r}")
        if name not in pins:
            raise ValueError(f"unregistered component {name}")
        digest, commit, plugin = pins[name]
        candidate = candidate_dir is not None and (candidate_dir / name).is_file()
        path = (candidate_dir if candidate else published_dir) / name
        if candidate:
            if name != "cpp_parser.wasm":
                raise ValueError("this tranche accepts only a C/C++ candidate component")
            if candidate_commit != commit:
                raise ValueError(f"candidate commit does not match registry for {name}")
            source = {"origin": "candidate_build", "commit": commit,
                      "repository": f"https://github.com/buchochelliq-labs/{plugin}"}
        else:
            record = provenance.get("artifacts", {}).get(name, {})
            source = dict(record.get("source", {}))
            if source.get("commit") != commit or record.get("sha256") != digest:
                raise ValueError(f"published source commit/checksum disagrees with registry for {name}")
            if source.get("repository") != f"https://github.com/buchochelliq-labs/{plugin}":
                raise ValueError(f"published repository disagrees with registry for {name}")
            source["origin"] = "published_wheel"
        try:
            payload = path.read_bytes()
        except OSError as error:
            raise ValueError(f"missing component {name}: {error}") from error
        if hashlib.sha256(payload).hexdigest() != digest:
            raise ValueError(f"component checksum mismatch for {name}")
        checked.append((name, payload, {"component": name, "sha256": digest,
                                      "registry_commit": registry_commit, **source}))
    # No partial manifest can certify a failed validation. Output is a fresh dir.
    if out.exists() and any(out.iterdir()):
        raise ValueError("output directory must be empty; refusing stale component inputs")
    out.mkdir(parents=True, exist_ok=True)
    for name, payload, _ in checked:
        (out / name).write_bytes(payload)
    manifest = {**inventory, "components": [record for _, _, record in checked]}
    (out / "parser_manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry", type=Path, required=True)
    parser.add_argument("--registry-commit", required=True)
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--published-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--candidate-dir", type=Path)
    parser.add_argument("--candidate-commit")
    args = parser.parse_args()
    try:
        stage(checked_registry(args.registry, args.registry_commit), json.loads(args.inventory.read_text()),
              args.published_dir, args.out, registry_commit=args.registry_commit,
              candidate_dir=args.candidate_dir, candidate_commit=args.candidate_commit)
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"benchmark provisioning failed: {error}\n")


if __name__ == "__main__":
    main()
