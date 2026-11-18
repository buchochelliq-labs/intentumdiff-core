"""Verify the benchmark's artifact inputs independently of parser execution."""
import hashlib
import importlib.util
import io
import zipfile
import json
import subprocess
from pathlib import Path

import pytest
import yaml

SCRIPT = Path(__file__).resolve().parents[2] / "scripts/stage_external_benchmark_inputs.py"


def load_module():
    spec = importlib.util.spec_from_file_location("external_staging", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def inputs(tmp_path):
    published = tmp_path / "published"
    published.mkdir()
    (published / "cpp_parser.wasm").write_bytes(b"old-component")
    digest = hashlib.sha256(b"old-component").hexdigest()
    inventory = {"schema_version": 1, "parsers": {"c": {"wasm": "cpp_parser.wasm", "plugin_id": "cpp", "extensions": [".c"]}}, "extension_index": {".c": "c"}}
    registry = {"version": 1, "plugins": {"intentumdiff-cpp-parser": {"ref": "a" * 40, "wasm_checksums": {"cpp_parser.wasm": digest}}}}
    provenance = {"artifacts": {"cpp_parser.wasm": {"sha256": digest, "source": {"commit": "a" * 40, "repository": "https://github.com/buchochelliq-labs/intentumdiff-cpp-parser"}}}}
    (published / "wasm_provenance.json").write_text(json.dumps(provenance))
    return published, inventory, registry


def test_stages_only_registry_verified_component_bytes(tmp_path):
    module = load_module()
    published, inventory, registry = inputs(tmp_path)
    output = tmp_path / "out"
    module.stage(registry, inventory, published, output, registry_commit="b" * 40)
    manifest = json.loads((output / "parser_manifest.json").read_text())
    assert (output / "cpp_parser.wasm").read_bytes() == b"old-component"
    assert manifest["parsers"] == inventory["parsers"]
    assert manifest["components"][0]["commit"] == "a" * 40
    assert manifest["components"][0]["registry_commit"] == "b" * 40


def test_rejects_missing_or_corrupted_components_before_writing_manifest(tmp_path):
    module = load_module()
    published, inventory, registry = inputs(tmp_path)
    (published / "cpp_parser.wasm").write_bytes(b"corrupted")
    with pytest.raises(ValueError, match="checksum"):
        module.stage(registry, inventory, published, tmp_path / "out", registry_commit="b" * 40)
    assert not (tmp_path / "out" / "parser_manifest.json").exists()


def test_candidate_requires_matching_registry_pin_and_commit(tmp_path):
    module = load_module()
    published, inventory, registry = inputs(tmp_path)
    payload = b"new-component"
    digest = hashlib.sha256(payload).hexdigest()
    registry["plugins"]["intentumdiff-cpp-parser"].update(
        ref="c" * 40, wasm_checksums={"cpp_parser.wasm": digest})
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as z:
        z.writestr("intentumdiff_cpp_parser.wasm", payload)
    blob = output.getvalue()
    artifacts = tmp_path / "artifacts"; artifacts.mkdir()
    (artifacts / "1.zip").write_bytes(blob)
    pin = {"repository": "buchochelliq-labs/intentumdiff-cpp-parser", "commit": "d" * 40,
           "artifact_id": 1, "archive_member": "intentumdiff_cpp_parser.wasm",
           "archive_sha256": hashlib.sha256(blob).hexdigest(), "component_sha256": digest}
    lock = {"schema_version": 2, "registry_commit": "b" * 40, "components": {"cpp_parser.wasm": pin}}
    with pytest.raises(ValueError, match="commit"):
        module.stage(registry, inventory, published, tmp_path / "out", registry_commit="b" * 40,
                     artifact_lock=lock, artifacts_dir=artifacts)
    pin["commit"] = "c" * 40
    module.stage(registry, inventory, published, tmp_path / "out", registry_commit="b" * 40,
                 artifact_lock=lock, artifacts_dir=artifacts)
    manifest = json.loads((tmp_path / "out" / "parser_manifest.json").read_text())
    assert manifest["components"][0]["origin"] == "locked_ci_archive"
    assert manifest["components"][0]["commit"] == "c" * 40


def test_cli_registry_identity_requires_exact_committed_bytes(tmp_path):
    module = load_module()
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    path = tmp_path / "registry.yaml"
    path.write_text("version: 1\nplugins: {}\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "registry.yaml"], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
    commit = subprocess.check_output(["git", "-C", str(tmp_path), "rev-parse", "HEAD"], text=True).strip()
    assert module.checked_registry(path, commit) == {"version": 1, "plugins": {}}
    path.write_text("version: 1\nplugins: {unreviewed: {}}\n")
    with pytest.raises(ValueError, match="bytes do not match"):
        module.checked_registry(path, commit)


@pytest.mark.parametrize("bad_name", ["../cpp_parser.wasm", "/tmp/cpp_parser.wasm", "sub/cpp_parser.wasm"])
def test_rejects_nonflat_component_names(tmp_path, bad_name):
    module = load_module()
    published, inventory, registry = inputs(tmp_path)
    inventory["parsers"]["c"]["wasm"] = bad_name
    with pytest.raises(ValueError):
        module.stage(registry, inventory, published, tmp_path / "out", registry_commit="b" * 40)
