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
