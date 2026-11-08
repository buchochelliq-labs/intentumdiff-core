"""The assessment must reject false success, independent of engine output."""
import importlib.util
from pathlib import Path

import pytest


def module():
    path = Path(__file__).resolve().parents[2] / 'scripts/assess_languages.py'
    spec = importlib.util.spec_from_file_location('assessment', path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def case(old='x = 1\n', new='x = 2\n'):
    return {'id': 'python-value', 'language_id': 'python', 'filename': 'a.py',
            'category': 'semantic_literal', 'old': old, 'new': new,
            'expected': {'has_semantic_changes': True, 'is_style_only': False,
                         'changes_empty': False}}


def review(**kwargs):
    return {'language': 'python', 'has_semantic_changes': True, 'is_style_only': False,
            'is_fallback': False, 'parse_errors': [], 'changes': [{'change_type': 'MODIFICATION'}],
            **kwargs}


def test_always_style_and_wrong_route_fail_meaningful_oracle():
    m = module()
    assert m.check_classification(case(), review(has_semantic_changes=False, is_style_only=True, changes=[]))
    assert m.check_classification(case(), review(language='javascript'))
    assert not m.check_classification(case(), review())


def test_exact_identity_rejects_invented_changes_and_semantics():
    m = module(); c = case('x = 1\n', 'x = 1\n')
    c['expected'] = {'has_semantic_changes': False, 'is_style_only': None, 'changes_empty': True}
    assert m.check_classification(c, review(has_semantic_changes=False, changes=[{'change_type': 'MOVE'}]))
    assert not m.check_classification(c, review(has_semantic_changes=False, is_style_only=True, changes=[]))


def test_fallback_or_missing_boolean_is_not_a_success():
    m = module()
    assert m.check_classification(case(), review(is_fallback=True))
    assert m.check_classification(case(), review(has_semantic_changes=None))


def test_range_validator_checks_utf8_boundaries_and_duplicate_ids():
    m = module(); source = 'λ = 1\n'
    tree = {'id': '0', 'position': {'start_line': 0, 'start_col': 0, 'end_line': 1, 'end_col': 0}, 'children': [
        {'id': '0', 'position': {'start_line': 0, 'start_col': 1, 'end_line': 0, 'end_col': 2}, 'children': []}]}
    errors = m.check_tree(tree, source)
    assert any('duplicate' in error for error in errors)
    assert any('UTF-8' in error for error in errors)


def test_normalization_keeps_semantics_but_ignores_recorded_timing():
    m = module()
    before = review(metadata={'duration_ms': 10, 'engine_telemetry': {'fuel_consumed': 4}})
    after = review(metadata={'duration_ms': 20, 'engine_telemetry': {'fuel_consumed': 8}})
    assert m.stable_review(before) == m.stable_review(after)
    after['has_semantic_changes'] = False
    assert m.stable_review(before) != m.stable_review(after)


def test_provenance_rejects_unpinned_external_claim():
    m = module(); c = case()
    c['provenance'] = {'class': 'upstream_gold', 'url': 'https://example.org/test', 'revision': 'main', 'license': 'MIT'}
    with pytest.raises(ValueError, match='immutable'):
        m.check_provenance(c, Path('/tmp'))


def test_actual_review_nodes_cannot_hide_invalid_ranges():
    m = module(); c = case()
    result = review(changes=[{'change_type': 'MODIFICATION', 'old_node': None, 'new_node': None}])
    assert m.check_review_nodes(result, c['old'], c['new'])
    node = {'id': '0', 'position': {'start_line': 90, 'start_col': 0, 'end_line': 90, 'end_col': 1}, 'children': []}
    result['changes'] = [{'change_type': 'MODIFICATION', 'old_node': node, 'new_node': node}]
    assert any('outside source' in e for e in m.check_review_nodes(result, c['old'], c['new']))


def test_lost_worker_does_not_turn_unobserved_tail_into_executions():
    m = module(); cases = [case(), {**case(), 'id': 'second'}]
    text = '{"type":"case","row":{"id":"python-value","status":"passed"}}\n'
    group = m.recover_worker(text, 'python', cases, 'timeout')
    assert group['completed_count'] == 1
    assert group['cases'][1]['status'] == 'not_executed'
    assert group['attempted_count'] is None


def test_incomplete_input_does_not_waive_route_or_range_validity():
    m = module(); c = case(); c['expected'] = dict.fromkeys(c['expected'])
    row = {'classification_errors': ['wrong route'], 'tree_errors': [], 'deterministic': True, 'parse_errors': []}
    assert m.case_status(c, row) == 'invalid_result'
    row['classification_errors'] = []; row['tree_errors'] = ['bad range']
    assert m.case_status(c, row) == 'invalid_result'
    row['tree_errors'] = []
    assert m.case_status(c, row) == 'bounded_incomplete_observation'


def test_parent_containment_is_observational_without_a_node_kind_contract():
    m = module()
    tree = {'id': '0', 'position': {'start_line': 0, 'start_col': 0, 'end_line': 0, 'end_col': 1},
            'children': [{'id': '0.0', 'position': {'start_line': 0, 'start_col': 4, 'end_line': 0, 'end_col': 5}, 'children': []}]}
    assert not m.check_tree(tree, 'x = 1')
    assert any('outside parent' in e for e in m.check_tree(tree, 'x = 1', containment=True))


def test_derived_fuel_observations_do_not_change_semantic_determinism():
    m = module()
    before = review(metadata={'engine_telemetry': {'fuel_hotspots': [{'fuel_used_percent': 1, 'fuel_per_line': 2, 'fuel_per_kb': 3}]}})
    after = review(metadata={'engine_telemetry': {'fuel_hotspots': [{'fuel_used_percent': 8, 'fuel_per_line': 9, 'fuel_per_kb': 10}]}})
    assert m.stable_review(before) == m.stable_review(after)


def observation(c=None, status='passed', **extra):
    m = module(); c = c or case()
    return {'id': c['id'], 'language_id': c['language_id'],
            'old_sha256': m.sha(c['old'].encode()), 'new_sha256': m.sha(c['new'].encode()),
            'expected': c['expected'], 'status': status, 'oracle_evaluated': status == 'passed', **extra}


def baseline(rows=None):
    return {'schema_version': 1, 'languages': [
        {'language_id': 'python', 'case_results': rows or [observation()]}]}


def collected(rows=None, **extra):
    return [{'language_id': 'python', 'cases': rows or [observation()],
             'completed_count': len(rows or [observation()]), 'worker_error': None,
             'native_library_sha256': 'candidate', **extra}]


def gate(b=None, cases=None, groups=None, selected=None):
    return module().compare_baseline(b or baseline(), cases if cases is not None else [case()],
                                     groups if groups is not None else collected(), selected, 'candidate')


@pytest.mark.parametrize('status', ['failed', 'runtime_error', 'not_executed', 'invalid_result'])
def test_baseline_gate_rejects_a_former_pass_with_unsuccessful_execution(status):
    result = gate(groups=collected([observation(status=status)]))
    assert result['regression_count'] > 0
    assert any(item.get('case_id') == 'python-value' for item in result['regressions'])


@pytest.mark.parametrize('change', ['missing_fixture', 'missing_result', 'old', 'new', 'expected'])
def test_baseline_gate_rejects_lost_coverage_and_changed_inputs(change):
    c = case(); cases = [c]; groups = collected()
    if change == 'missing_fixture':
        cases = []
    elif change == 'missing_result':
        groups = []
    elif change == 'expected':
        c['expected'] = {**c['expected'], 'has_semantic_changes': False}
    else:
        c[change] += '# drift\n'
    result = gate(cases=cases, groups=groups)
    assert result['regression_count'] > 0
    assert any(item.get('case_id') == 'python-value' for item in result['regressions'])


def test_baseline_gate_checks_input_drift_even_for_known_failures():
    c = case(); c['new'] += '# changed fixture\n'
    assert gate(b=baseline([observation(status='failed')]), cases=[c])['regression_count']


@pytest.mark.parametrize('status', ['failed', 'invalid_result'])
def test_baseline_gate_keeps_known_failures_and_improvements_visible(status):
    b = baseline([observation(status=status)])
    assert gate(b=b, groups=collected([observation(status=status)]))['regression_count'] == 0
    improved = gate(b=b)
    assert improved['regression_count'] == 0
    assert improved['improvements'] == ['python-value']


def test_baseline_gate_does_not_require_historical_native_or_component_hashes():
    b = {**baseline(), 'native_library_sha256': 'old', 'parser_manifest_sha256': 'old-components'}
    assert gate(b=b)['regression_count'] == 0


@pytest.mark.parametrize('extra', [{'worker_error': 'timeout'}, {'native_library_sha256': 'wrong'}])
def test_baseline_gate_rejects_worker_loss_or_wrong_library_even_after_last_pass(extra):
    assert gate(groups=collected(**extra))['regression_count'] > 0


def test_baseline_gate_rejects_runtime_errors_in_new_observations():
    new = {**case(), 'id': 'new-observation'}
    assert gate(cases=[case(), new], groups=collected([observation(), observation(new, 'runtime_error')]))['regression_count']


def test_explicit_language_selection_limits_baseline_scope_only_when_requested():
    other = {**observation(), 'id': 'rust-pass', 'language_id': 'rust'}
    b = baseline(); b['languages'].append({'language_id': 'rust', 'case_results': [other]})
    assert gate(b=b)['regression_count'] > 0
    assert gate(b=b, selected=['python'])['regression_count'] == 0


def test_native_selection_uses_explicit_path_without_replacing_installed_library(tmp_path, monkeypatch):
    import intentumdiff.rust_core as transport
    m = module(); native = tmp_path / 'candidate.so'; native.write_bytes(b'candidate artifact')
    installed = transport._extension_path()
    monkeypatch.setattr(transport, '_extension_path', transport._extension_path)
    monkeypatch.setattr(transport, '_C_ABI_LIB', object())
    selected, path = m.worker_transport(str(native))
    assert selected is transport
    assert path == native.resolve()
    assert selected._extension_path() == str(native.resolve())
    assert selected._C_ABI_LIB is None
    assert m.sha(path.read_bytes()) == m.sha(b'candidate artifact')
    assert Path(installed).is_file()


def test_native_default_selection_keeps_installed_transport(monkeypatch):
    import intentumdiff.rust_core as transport
    m = module(); original = transport._extension_path; loaded = object()
    monkeypatch.setattr(transport, '_C_ABI_LIB', loaded)
    selected, path = m.worker_transport(None)
    assert path == Path(original()).resolve()
    assert selected._extension_path is original
    assert selected._C_ABI_LIB is loaded


@pytest.mark.parametrize('baseline_kind', [None, 'valid', 'malformed_json', 'empty_cases', 'duplicate_case'])
def test_cli_retains_real_transport_load_failure_and_default_collector_exit(tmp_path, baseline_kind):
    """No fake semantic verdict: a non-library must fail the actual ctypes loader."""
    import json
    import subprocess
    import sys
    m = module(); root = Path(__file__).resolve().parents[2]
    inventory = json.loads((root / 'tests/fixtures/external/inventory.json').read_text())
    parsers = tmp_path / 'parsers'; parsers.mkdir()
    components = []
    for name in sorted({entry['wasm'] for entry in inventory['parsers'].values()}):
        (parsers / name).write_bytes(b'not reached: native library cannot load')
        components.append({'component': name, 'sha256': m.sha((parsers / name).read_bytes())})
    (parsers / 'parser_manifest.json').write_text(json.dumps({**inventory, 'components': components}))
    fixtures = tmp_path / 'fixtures'; fixtures.mkdir()
    c = {**case(), 'provenance': {'class': 'handwritten_regression'}, 'oracle': {'basis': 'changed literal'}}
    (fixtures / 'one.json').write_text(json.dumps({'schema_version': 1, 'languages': ['python'], 'cases': [c]}))
    native = tmp_path / 'not-a-library.so'; native.write_bytes(b'not an ELF library')
    report = tmp_path / 'report/results.json'; baseline_path = tmp_path / 'baseline.json'
    b = baseline()
    if baseline_kind == 'empty_cases':
        b['languages'][0]['case_results'] = []
    elif baseline_kind == 'duplicate_case':
        b['languages'][0]['case_results'].append(observation())
    baseline_path.write_text('{broken JSON' if baseline_kind == 'malformed_json' else json.dumps(b))
    command = [sys.executable, str(root / 'scripts/assess_languages.py'), '--fixtures', str(fixtures),
               '--parsers', str(parsers), '--report', str(report), '--languages', 'python',
               '--native-library', str(native)]
    if baseline_kind is not None:
        command += ['--baseline', str(baseline_path)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
    assert result.returncode == int(baseline_kind is not None), result.stderr
    doc = json.loads(report.read_text()); raw = json.loads((report.parent / 'raw/python.json').read_text())
    assert doc['languages'][0]['case_results'][0]['status'] == 'runtime_error'
    assert 'OSError' in raw['cases'][0]['error']
    assert doc['native_library_sha256'] == raw['native_library_sha256'] == m.sha(native.read_bytes())
    assert doc['native_library_path'] == raw['native_library_path'] == str(native.resolve())
    if baseline_kind is not None:
        assert doc['baseline_gate']['regression_count'] > 0
        if baseline_kind != 'valid':
            assert doc['baseline_gate']['error']
    else:
        assert 'baseline_gate' not in doc


def test_worker_launch_error_is_retained_as_unexecuted_coverage(tmp_path, monkeypatch):
    from types import SimpleNamespace
    m = module()

    def cannot_launch(*args, **kwargs):
        raise OSError('process creation unavailable')

    monkeypatch.setattr(m.subprocess, 'run', cannot_launch)
    group = m.run_group('python', [case()], SimpleNamespace(parsers=tmp_path, native_library=None, timeout=1))
    assert group['completed_count'] == 0
    assert group['cases'][0]['status'] == 'not_executed'
    assert 'process creation unavailable' in group['worker_error']


@pytest.mark.parametrize('corruption', [
    'empty_cases', 'duplicate_language', 'duplicate_case', 'unknown_status', 'missing_status',
    'wrong_hash_type', 'invalid_hash', 'invalid_expected', 'missing_expected_key',
    'invalid_languages_type', 'misrouted_case', 'malformed_unselected_language',
])
def test_corrupt_baseline_cannot_claim_a_successful_gate(corruption):
    b = baseline(); row = b['languages'][0]['case_results'][0]
    if corruption == 'empty_cases':
        b['languages'][0]['case_results'] = []
    elif corruption == 'duplicate_language':
        b['languages'].append({'language_id': 'python', 'case_results': [observation()]})
    elif corruption == 'duplicate_case':
        b['languages'][0]['case_results'].append(observation())
    elif corruption == 'unknown_status':
        row['status'] = 'typo'
    elif corruption == 'missing_status':
        del row['status']
    elif corruption == 'wrong_hash_type':
        row['old_sha256'] = 42
    elif corruption == 'invalid_hash':
        row['new_sha256'] = 'not-a-source-hash'
    elif corruption == 'invalid_expected':
        row['expected']['has_semantic_changes'] = 'true'
    elif corruption == 'missing_expected_key':
        del row['expected']['is_style_only']
    elif corruption == 'invalid_languages_type':
        b['languages'] = {'python': []}
    elif corruption == 'misrouted_case':
        row['language_id'] = 'rust'
    else:
        b['languages'].append({'language_id': 'rust', 'case_results': []})
    with pytest.raises(ValueError, match='baseline'):
        gate(b=b, selected=['python'])


def test_baseline_must_cover_every_requested_language():
    with pytest.raises(ValueError, match='baseline.*coverage'):
        gate(selected=['rust'])


def test_baseline_must_cover_every_current_language_without_scope_filter():
    c = {**case(), 'id': 'rust-case', 'language_id': 'rust'}
    with pytest.raises(ValueError, match='baseline.*coverage'):
        gate(cases=[case(), c])
