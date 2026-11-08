"""Reviewed CI archives must preserve the registry's two independent pins."""
import hashlib
import io
import json
import runpy
import subprocess
import zipfile
import urllib.request
from pathlib import Path

import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
STAGER = ROOT / "scripts/stage_external_benchmark_inputs.py"
PROVISION = ROOT / "scripts/provision_parser_components.py"


def archive_bytes(members):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        for name, payload in members:
            archive.writestr(name, payload)
    return output.getvalue()


def candidate_inputs(tmp_path):
    registry = {"version": 1, "plugins": {}}
    inventory = {"schema_version": 1, "parsers": {}, "extension_index": {}}
    lock = {"schema_version": 2, "registry_commit": "a" * 40,
            "published_distribution": "intentumdiff-python==0.0.2b2", "components": {}}
    archives = tmp_path / "archives"; archives.mkdir()
    published = tmp_path / "published"; published.mkdir()
    (published / "wasm_provenance.json").write_text(json.dumps({"artifacts": {}}))
    for index, slug in enumerate(["cpp", "js-ts", "json", "sql", "yaml"], 1):
        name = slug.replace("-", "_") + "_parser.wasm"
        payload = (slug + " genuine component").encode()
        archive = archive_bytes([("intentumdiff_" + name, payload)])
        commit = str(index) * 40
        component_sha = hashlib.sha256(payload).hexdigest()
        registry["plugins"][f"intentumdiff-{slug}-parser"] = {
            "ref": commit, "wasm_checksums": {name: component_sha}}
        inventory["parsers"][slug] = {"wasm": name, "plugin_id": slug, "extensions": ["." + slug]}
        inventory["extension_index"]["." + slug] = slug
        lock["components"][name] = {"repository": f"buchochelliq-labs/intentumdiff-{slug}-parser",
            "commit": commit, "artifact_id": index, "archive_member": "intentumdiff_" + name,
            "archive_sha256": hashlib.sha256(archive).hexdigest(), "component_sha256": component_sha}
        (archives / f"{index}.zip").write_bytes(archive)
    return registry, inventory, lock, archives, published


def test_cli_stages_all_five_exact_ci_archives(tmp_path):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    root = tmp_path / "registry"; root.mkdir()
    subprocess.run(["git", "init", "-q", str(root)], check=True)
    (root / "registry.yaml").write_text(yaml.safe_dump(registry))
    subprocess.run(["git", "-C", str(root), "add", "registry.yaml"], check=True)
    subprocess.run(["git", "-C", str(root), "-c", "user.name=Fixture", "-c",
                    "user.email=fixture@example.invalid", "commit", "-qm", "registry"], check=True)
    commit = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    lock["registry_commit"] = commit
    lock_path = tmp_path / "inputs.json"; lock_path.write_text(json.dumps(lock))
    inventory_path = tmp_path / "inventory.json"; inventory_path.write_text(json.dumps(inventory))
    output = tmp_path / "out"
    result = subprocess.run([__import__("sys").executable, str(STAGER), "--registry", str(root / "registry.yaml"),
        "--registry-commit", commit, "--inventory", str(inventory_path), "--published-dir", str(published),
        "--lock", str(lock_path), "--artifacts-dir", str(archives), "--out", str(output)], capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    manifest = json.loads((output / "parser_manifest.json").read_text())
    assert {row["component"] for row in manifest["components"]} == set(lock["components"])
    assert manifest["parsers"] == inventory["parsers"]
    for row in manifest["components"]:
        pin = lock["components"][row["component"]]
        assert row["commit"] == pin["commit"] and row["sha256"] == pin["component_sha256"]
        assert row["artifact_id"] == pin["artifact_id"]
        assert row["artifact_archive_sha256"] == pin["archive_sha256"]
        assert row["registry_commit"] == commit
        assert row["origin"] == "locked_ci_archive"


@pytest.mark.parametrize("field,value", [
    ("commit", "f" * 40), ("component_sha256", "f" * 64),
    ("repository", "other-org/intentumdiff-json-parser"), ("artifact_id", True),
    ("archive_sha256", "mutable"), ("archive_member", "../intentumdiff_json_parser.wasm"),
])
def test_locked_component_cannot_disagree_with_registry(tmp_path, field, value):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    lock["components"]["json_parser.wasm"][field] = value
    stage = runpy.run_path(str(STAGER))["stage"]
    output = tmp_path / "out"
    with pytest.raises(ValueError):
        stage(registry, inventory, published, output, registry_commit=lock["registry_commit"],
              artifact_lock=lock, artifacts_dir=archives)
    assert not output.exists()


@pytest.mark.parametrize("fault", ["wrong_archive", "wrong_payload", "duplicate", "nested", "extra", "symlink", "backslash", "traversal"])
def test_bad_archive_never_leaves_a_partially_certified_output(tmp_path, fault):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    pin = lock["components"]["yaml_parser.wasm"]
    member = pin["archive_member"]
    payload = b"yaml genuine component"
    if fault == "wrong_archive":
        blob = b"not the locked archive"
    else:
        members = [(member, payload)]
        if fault == "wrong_payload": members = [(member, b"corrupt")]
        if fault == "duplicate": members += [(member, payload)]
        if fault == "nested": members = [("nested/" + member, payload)]
        if fault == "extra": members += [("unexpected.txt", b"extra")]
        if fault == "backslash": members = [("nested\\" + member, payload)]
        if fault == "traversal": members = [("../" + member, payload)]
        if fault == "symlink":
            info = zipfile.ZipInfo(member); info.external_attr = (0o120777 << 16)
            members = [(info, payload)]
        if fault == "duplicate":
            with pytest.warns(UserWarning, match="Duplicate name"):
                blob = archive_bytes(members)
        else:
            blob = archive_bytes(members)
        pin["archive_sha256"] = hashlib.sha256(blob).hexdigest()
    (archives / f"{pin['artifact_id']}.zip").write_bytes(blob)
    stage = runpy.run_path(str(STAGER))["stage"]
    output = tmp_path / "out"
    with pytest.raises(ValueError):
        stage(registry, inventory, published, output, registry_commit=lock["registry_commit"],
              artifact_lock=lock, artifacts_dir=archives)
    assert not output.exists()


def test_missing_locked_archive_cannot_fall_back_to_an_older_wheel(tmp_path):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    pin = lock["components"]["json_parser.wasm"]
    (archives / f"{pin['artifact_id']}.zip").unlink()
    (published / "json_parser.wasm").write_bytes(b"old wheel bytes")
    stage = runpy.run_path(str(STAGER))["stage"]
    with pytest.raises(ValueError, match="archive"):
        stage(registry, inventory, published, tmp_path / "out", registry_commit=lock["registry_commit"],
              artifact_lock=lock, artifacts_dir=archives)


@pytest.mark.parametrize("fault", ["id", "expired", "head", "name", "run", "run_id", "run_head", "incomplete"])
def test_download_rejects_wrong_remote_artifact_identity_before_publication(tmp_path, monkeypatch, fault):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    module = runpy.run_path(str(PROVISION))
    fetch = module["fetch_locked_artifact"]
    pin = lock["components"]["json_parser.wasm"]
    artifact = {"id": pin["artifact_id"], "name": "parser-wasm", "expired": False,
                "workflow_run": {"id": 10, "head_sha": pin["commit"]}}
    run = {"id": 10, "head_sha": pin["commit"], "status": "completed", "conclusion": "success"}
    if fault == "id": artifact["id"] += 1
    if fault == "expired": artifact["expired"] = True
    if fault == "head": artifact["workflow_run"]["head_sha"] = "e" * 40
    if fault == "name": artifact["name"] = "unreviewed-artifact"
    if fault == "run": run["conclusion"] = "failure"
    if fault == "run_id": run["id"] += 1
    if fault == "run_head": run["head_sha"] = "e" * 40
    if fault == "incomplete": run["status"] = "in_progress"
    def get(url, token, **kwargs):
        assert url.startswith("https://api.github.com/repos/" + pin["repository"] + "/actions/")
        if url.endswith("/zip"): return (archives / f"{pin['artifact_id']}.zip").read_bytes()
        return json.dumps(run if "/runs/" in url else artifact).encode()
    monkeypatch.setitem(fetch.__globals__, "_get", get)
    with pytest.raises(ValueError):
        fetch("json_parser.wasm", pin, "test-token")


def test_download_uses_fixed_api_urls_instead_of_metadata_urls(tmp_path, monkeypatch):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    fetch = runpy.run_path(str(PROVISION))["fetch_locked_artifact"]
    pin = lock["components"]["json_parser.wasm"]
    base = "https://api.github.com/repos/" + pin["repository"] + "/actions"
    seen = []
    def get(url, token, **kwargs):
        seen.append(url)
        if url.endswith("/zip"):
            return (archives / f"{pin['artifact_id']}.zip").read_bytes()
        if "/runs/" in url:
            return json.dumps({"id": 10, "head_sha": pin["commit"], "status": "completed",
                               "conclusion": "success"}).encode()
        return json.dumps({"id": pin["artifact_id"], "name": "parser-wasm", "expired": False,
            "archive_download_url": "https://untrusted.invalid/steal-token", "workflow_run": {
                "id": 10, "head_sha": pin["commit"], "url": "https://untrusted.invalid/run"}}).encode()
    monkeypatch.setitem(fetch.__globals__, "_get", get)
    assert fetch("json_parser.wasm", pin, "test-token") == (archives / f"{pin['artifact_id']}.zip").read_bytes()
    assert seen == [f"{base}/artifacts/{pin['artifact_id']}", f"{base}/runs/10",
                    f"{base}/artifacts/{pin['artifact_id']}/zip"]


def test_cross_host_redirect_drops_auth_and_downgrade_is_rejected():
    handler = runpy.run_path(str(PROVISION))["_StripAuthOnRedirect"]()
    request = urllib.request.Request("https://api.github.com/archive", headers={"Authorization": "Bearer secret"})
    redirected = handler.redirect_request(request, None, 302, "redirect", {}, "https://blob.example/archive")
    assert redirected.get_header("Authorization") is None
    second = handler.redirect_request(redirected, None, 302, "redirect", {}, "https://api.github.com/again")
    assert second.get_header("Authorization") is None
    with pytest.raises(__import__("urllib.error", fromlist=["HTTPError"]).HTTPError):
        handler.redirect_request(request, None, 302, "redirect", {}, "http://api.github.com/archive")


@pytest.mark.parametrize("failure", ["write", "rename"])
def test_atomic_publication_failure_leaves_no_manifest_or_partial_components(tmp_path, monkeypatch, failure):
    module = runpy.run_path(str(PROVISION))
    publish = module["publish_files"]
    output = tmp_path / "out"
    if failure == "rename":
        def broken(*args): raise OSError("simulated rename failure")
        monkeypatch.setattr(publish.__globals__["os"], "replace", broken)
    else:
        original = Path.write_bytes
        def write(path, payload):
            if path.name == "parser_manifest.json": raise OSError("simulated disk failure")
            return original(path, payload)
        monkeypatch.setattr(Path, "write_bytes", write)
    with pytest.raises(OSError):
        publish(output, {"first.wasm": b"first", "parser_manifest.json": b"manifest"})
    assert not output.exists()
    assert not list(tmp_path.glob(".out-*"))


def test_locked_provisioning_precedes_wheel_and_latest_run_paths(tmp_path, monkeypatch):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    main = runpy.run_path(str(PROVISION))["main"]
    pin = lock["components"]["json_parser.wasm"]
    monkeypatch.setenv("GH_TOKEN", "test-token")
    monkeypatch.setattr("sys.argv", ["provision", "--components", "json", "--out", str(tmp_path / "out"),
                                    "--mirror-dir", str(published)])
    monkeypatch.setitem(main.__globals__, "LOCKED_COMPONENTS", {"json_parser.wasm": pin})
    monkeypatch.setitem(main.__globals__, "load_registry_pins", lambda token: (
        {"json_parser.wasm": pin["component_sha256"]}, {"intentumdiff-json-parser": pin["commit"]}))
    def forbidden(*args): raise AssertionError("locked artifact cannot use a mirror or latest run")
    monkeypatch.setitem(main.__globals__, "fetch_component", forbidden)
    monkeypatch.setitem(main.__globals__, "fetch_mirrored_component", forbidden)
    monkeypatch.setitem(main.__globals__, "fetch_locked_artifact", lambda *args: (archives / f"{pin['artifact_id']}.zip").read_bytes())
    main()
    assert (tmp_path / "out/json_parser.wasm").read_bytes() == b"json genuine component"
    manifest = json.loads((tmp_path / "out/parser_component_provenance.json").read_text())
    assert manifest["components"][0]["artifact_id"] == pin["artifact_id"]


def test_stale_output_and_reserved_source_fields_cannot_recertify_other_bytes(tmp_path):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    payload = b"unchanged Go parser"; digest = hashlib.sha256(payload).hexdigest()
    registry["plugins"]["intentumdiff-go-parser"] = {"ref": "e" * 40,
        "wasm_checksums": {"go_parser.wasm": digest}}
    inventory["parsers"]["go"] = {"wasm": "go_parser.wasm", "plugin_id": "go", "extensions": [".go"]}
    inventory["extension_index"][".go"] = "go"
    (published / "go_parser.wasm").write_bytes(payload)
    source = {"commit": "e" * 40, "repository": "https://github.com/buchochelliq-labs/intentumdiff-go-parser",
              "component": "evil.wasm", "sha256": "f" * 64, "registry_commit": "f" * 40,
              "origin": "fake_certificate"}
    (published / "wasm_provenance.json").write_text(json.dumps({"artifacts": {
        "go_parser.wasm": {"sha256": digest, "source": source}}}))
    stage = runpy.run_path(str(STAGER))["stage"]
    output = tmp_path / "out"
    stage(registry, inventory, published, output, registry_commit=lock["registry_commit"],
          artifact_lock=lock, artifacts_dir=archives)
    manifest = json.loads((output / "parser_manifest.json").read_bytes())
    row = next(row for row in manifest["components"] if row["component"] == "go_parser.wasm")
    assert row["sha256"] == digest and row["registry_commit"] == lock["registry_commit"]
    assert row["origin"] == "published_wheel"
    before = {p.name: p.read_bytes() for p in output.iterdir()}
    with pytest.raises(ValueError, match="stale"):
        stage(registry, inventory, published, output, registry_commit=lock["registry_commit"],
              artifact_lock=lock, artifacts_dir=archives)
    assert before == {p.name: p.read_bytes() for p in output.iterdir()}


def test_failed_last_download_does_not_publish_earlier_archives(tmp_path, monkeypatch):
    registry, inventory, lock, archives, published = candidate_inputs(tmp_path)
    download = runpy.run_path(str(PROVISION))["download_locked_artifacts"]
    seen = []
    def fetch(name, pin, token):
        seen.append(name)
        if name == "yaml_parser.wasm": raise ValueError("last archive failed verification")
        return (archives / f"{pin['artifact_id']}.zip").read_bytes()
    monkeypatch.setitem(download.__globals__, "fetch_locked_artifact", fetch)
    output = tmp_path / "out"
    with pytest.raises(ValueError, match="verification"):
        download(lock["components"], "test-token", output)
    assert len(seen) == 5
    assert not output.exists()
