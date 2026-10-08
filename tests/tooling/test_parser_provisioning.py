"""Pinned builds remain discoverable after unrelated workflow runs accumulate."""
import json
import runpy
from pathlib import Path


def test_pinned_component_survives_more_than_one_page_of_other_workflows(monkeypatch):
    stage = runpy.run_path(str(Path(__file__).resolve().parents[2] / 'scripts/provision_parser_components.py'))['_successful_runs']
    seen = []
    def get(url, token):
        seen.append(url)
        assert 'head_sha=pinned' in url
        assert 'status=success' in url
        # Simulate the repository's history: a full page of scheduled jobs,
        # then the successful build of the same pinned commit.
        if '&page=2' in url:
            return json.dumps({'workflow_runs': [{'id': 101, 'head_sha': 'pinned'}]}).encode()
        return json.dumps({'workflow_runs': [{'id': i, 'head_sha': 'pinned'} for i in range(100)]}).encode()
    monkeypatch.setitem(stage.__globals__, '_get', get)
    assert any(run['id'] == 101 for run in stage('intentumdiff-python-parser', 'test-token', 'pinned'))
    assert len(seen) == 2


def test_history_lookup_never_accepts_a_different_commit(monkeypatch):
    stage = runpy.run_path(str(Path(__file__).resolve().parents[2] / 'scripts/provision_parser_components.py'))['_successful_runs']
    monkeypatch.setitem(stage.__globals__, '_get', lambda *args: json.dumps({'workflow_runs': [
        {'id': 1, 'head_sha': 'other'}, {'id': 2, 'head_sha': 'pinned'}]}).encode())
    assert stage('parser', '', 'pinned') == [{'id': 2, 'head_sha': 'pinned'}]


def test_registry_document_is_fetched_at_candidate_commit(monkeypatch):
    load = runpy.run_path(str(Path(__file__).resolve().parents[2] / 'scripts/provision_parser_components.py'))['load_registry_pins']
    seen = []
    def get(url, token, **kwargs):
        seen.append(url)
        return b'plugins:\n  intentumdiff-json-parser:\n    ref: pinned\n    wasm_checksums:\n      json_parser.wasm: digest\n'
    monkeypatch.setitem(load.__globals__, '_get', get)
    assert load('test-token') == ({'json_parser.wasm': 'digest'}, {'intentumdiff-json-parser': 'pinned'})
    assert seen == ['https://api.github.com/repos/buchochelliq-labs/intentumdiff-registry/contents/registry.yaml?ref=59d7c91fa4e466b4130628202e6a126792b6f9d7']


def test_default_provisioning_stages_checksum_verified_json(monkeypatch, tmp_path):
    import hashlib
    stage = runpy.run_path(str(Path(__file__).resolve().parents[2] / 'scripts/provision_parser_components.py'))['main']
    payload = b'test component bytes'
    digest = hashlib.sha256(payload).hexdigest()
    calls = []
    slugs = ['python', 'go', 'js-ts', 'ini', 'asm', 'json']
    pins = {f"{slug.replace('-', '_')}_parser.wasm": digest for slug in slugs}
    refs = {f'intentumdiff-{slug}-parser': f'pinned-{slug}' for slug in slugs}
    monkeypatch.setenv('GH_TOKEN', 'test-token')
    monkeypatch.setattr('sys.argv', ['provision', '--out', str(tmp_path)])
    monkeypatch.setitem(stage.__globals__, 'load_registry_pins', lambda token: (pins, refs))
    def fetch(slug, token, ref):
        calls.append(slug)
        assert ref == f'pinned-{slug}'
        return f"{slug.replace('-', '_')}_parser.wasm", ref, digest, payload
    monkeypatch.setitem(stage.__globals__, 'fetch_component', fetch)
    stage()
    assert set(calls) == set(slugs)
    assert (tmp_path / 'json_parser.wasm').read_bytes() == payload


def test_json_checksum_mismatch_never_reaches_staging(monkeypatch, tmp_path):
    import pytest
    stage = runpy.run_path(str(Path(__file__).resolve().parents[2] / 'scripts/provision_parser_components.py'))['main']
    monkeypatch.setenv('GH_TOKEN', 'test-token')
    monkeypatch.setattr('sys.argv', ['provision', '--out', str(tmp_path), '--components', 'json'])
    monkeypatch.setitem(stage.__globals__, 'load_registry_pins', lambda token: (
        {'json_parser.wasm': 'expected'}, {'intentumdiff-json-parser': 'pinned-json'}))
    monkeypatch.setitem(stage.__globals__, 'fetch_component', lambda *args: (
        'json_parser.wasm', 'pinned-json', 'wrong', b'unverified bytes'))
    with pytest.raises(SystemExit, match='1'):
        stage()
    assert not (tmp_path / 'json_parser.wasm').exists()
