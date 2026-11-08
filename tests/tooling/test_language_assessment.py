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
