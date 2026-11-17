"""#265 structured family: parser-fidelity lane, blocked acquisitions and fragment reproducibility."""
import copy
import importlib.util
import json
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).resolve().parents[2] / 'scripts'
REAL = SCRIPTS.parent / 'tests' / 'fixtures' / 'external'


def _load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


L = _load('external_ledger', 'external_ledger.py')
T = _load('test_external_ledger_helpers', '../tests/tooling/test_external_ledger.py')
STRUCTURED = ('yaml', 'json', 'toml', 'xml', 'ini', 'proto', 'graphql')


def _parser_root(tmp_path, accepts=True):
    root, ledger = T.make_root(tmp_path)
    doc = T._ref(root, 'cases/doc.cpp', b'int x;\n')
    case = copy.deepcopy(ledger['cases'][1])
    case.update(id='parse', lane='parser_fidelity', change_family='accept' if accepts else 'reject', old=doc, new=doc,
                derivation=[{'step': 'verbatim_upstream'}], **{'class': 'upstream_gold'})
    case['oracle']['witness'] = T._witness(root, 'parse', doc, doc, 'supports_claim')
    if accepts:
        case['expected'] = {'classification': {'has_semantic_changes': False, 'is_style_only': None,
                                               'changes_empty': True, 'completeness': None},
                            'required': [], 'alternatives': [], 'forbidden': [], 'parse': {'accepts': True}}
    else:
        case['expected'] = {'classification': {'has_semantic_changes': None, 'is_style_only': None,
                                               'changes_empty': None, 'completeness': None},
                            'required': [], 'alternatives': [], 'forbidden': [], 'parse': {'accepts': False}}
    ledger['cases'].append(case)
    ledger['discovery'].append({'suite_id': 'hep', 'language_id': 'cpp', 'lane': 'parser_fidelity',
                                'discovered': 1, 'eligible': 1, 'eligibility_rule': 'r'})
    return root, ledger, case


def test_checked_in_fragment_is_reproducible_and_merged():
    family = _load('structured_family', 'structured_family.py')
    family.CHECK_ONLY = True  # never rewrite fixtures from a test
    assert L.encode(family.build()) == family.FRAGMENT.read_bytes(), 'structured fragment is stale'
    ledger, _ = L.validate(REAL)
    fragment = json.loads(family.FRAGMENT.read_bytes())
    ids = {c['id'] for c in ledger['cases']}
    assert {c['id'] for c in fragment['cases']} <= ids
    assert {c['language_id'] for c in fragment['cases']} == set(STRUCTURED) - {'xml'}
    for case in fragment['cases']:
        if case['admission']['status'] == 'admitted':
            assert case['oracle']['witness'] is not None
        if case['lane'] == 'parser_fidelity':
            assert case['old'] == case['new']


def test_parser_fidelity_cases_validate(tmp_path):
    for accepts in (True, False):
        root, ledger, _ = _parser_root(tmp_path / str(accepts), accepts)
        L.validate(root, ledger)


@pytest.mark.parametrize('mutate, message', [
    (lambda c: c['expected'].pop('parse'), 'parse.accepts'),
    (lambda c: c['expected']['parse'].update(accepts='yes'), 'parse.accepts'),
    (lambda c: c.update(new=c['old'] | {'path': 'cases/new.cpp', 'sha256': L.sha(b"char f(){return '1';}\n")}),
     'old == new|witness record'),
])
def test_parser_fidelity_rejects(tmp_path, mutate, message):
    root, ledger, case = _parser_root(tmp_path)
    mutate(case)
    with pytest.raises(L.LedgerError, match=message):
        L.validate(root, ledger)


def test_must_reject_case_cannot_constrain_review(tmp_path):
    root, ledger, case = _parser_root(tmp_path, accepts=False)
    case['expected']['classification']['has_semantic_changes'] = False
    with pytest.raises(L.LedgerError, match='must-reject'):
        L.validate(root, ledger)


def test_runner_parse_adapter(tmp_path):
    R = _load('run_external_ledger', 'run_external_ledger.py')
    root, ledger, accept = _parser_root(tmp_path / 'a')
    clean = {'status': 'passed', 'classification_errors': [], 'tree_errors': [], 'review_errors': [],
             'parse_errors': [], 'deterministic': True,
             'review': {'has_semantic_changes': False, 'is_style_only': True, 'is_fallback': False, 'changes': [],
                        'completeness': 'complete'}}
    assert R.classify(accept, clean, root) == ('passed', [])
    rejected = {'status': 'failed', 'explicit_fallback': {'fallback': 'x'}}
    assert R.classify(accept, rejected, root)[0] == 'failed'      # valid input rejected is a failure, not unsupported
    assert R.classify(accept, dict(clean, parse_errors=['e']), root)[0] == 'failed'
    root, ledger, reject = _parser_root(tmp_path / 'r', accepts=False)
    assert R.classify(reject, {'status': 'bounded_explicit_fallback'}, root) == ('passed', [])
    assert R.classify(reject, {'status': 'bounded_explicit_error'}, root) == ('passed', [])
    assert R.classify(reject, {'status': 'bounded_incomplete_observation', 'handling': 'diagnostics_present'},
                      root) == ('passed', [])
    status, errors = R.classify(reject, {'status': 'bounded_incomplete_observation',
                                         'handling': 'accepted_without_diagnostics'}, root)
    assert status == 'failed' and 'without diagnostics' in errors[0]
    assert R.classify(reject, {'status': 'invalid_result', 'tree_errors': ['bad range']}, root)[0] == 'failed'
    assert R.classify(reject, {'status': 'not_executed'}, root)[0] == 'blocked'


def test_blocked_acquisition_is_reported_never_measured(tmp_path):
    root, ledger = T.make_root(tmp_path)
    ledger['blocked_acquisitions'] = [{'language_id': 'zig', 'suite_id': 's', 'name': 'Suite', 'url': 'https://x',
                                       'lanes': ['parser_fidelity'], 'reason': 'host unreachable'}]
    L.validate(root, ledger)
    selection = json.loads((root / 'ledger.json').read_bytes())
    card = L.build_scorecard(ledger, json.loads((root / 'inventory.json').read_bytes()), selection, None)
    zig = next(e for e in card['languages'] if e['language_id'] == 'zig')
    assert zig['lanes']['parser_fidelity']['status'] == 'blocked'
    assert 'host unreachable' in zig['lanes']['parser_fidelity']['na_reason']
    assert zig['lanes']['parser_fidelity']['external_gold_pass_rate']['value'] is None
    assert zig['lanes']['change_understanding']['status'] == 'gap'
    bad = copy.deepcopy(ledger); bad['blocked_acquisitions'][0]['lanes'] = ['vibes']
    with pytest.raises(L.LedgerError, match='unknown language or lane'):
        L.validate(root, bad)


def test_real_scorecard_has_every_structured_id_and_xml_is_blocked():
    ledger, inventory = L.validate(REAL)
    selection = json.loads((REAL / 'ledger.json').read_bytes())
    card = L.build_scorecard(ledger, inventory, selection, None)
    by_id = {e['language_id']: e for e in card['languages']}
    assert set(STRUCTURED) <= set(by_id)
    for lane in ('parser_fidelity', 'change_understanding'):
        assert by_id['xml']['lanes'][lane]['status'] == 'blocked'
    # Without a runner report every admitted case is skipped: no lane can be measured.
    assert all(l['status'] != 'measured' for e in card['languages'] for l in e['lanes'].values())


def test_parser_lane_scores_acceptance_and_rejection_not_change_facts(tmp_path):
    root, ledger, accept = _parser_root(tmp_path)
    reject = copy.deepcopy(accept)
    reject.update(id='parse-bad', change_family='reject')
    reject['expected'] = {'classification': {k: None for k in ('has_semantic_changes', 'is_style_only', 'changes_empty',
                                                               'completeness')},
                          'required': [], 'alternatives': [], 'forbidden': [], 'parse': {'accepts': False}}
    reject['oracle']['witness'] = T._witness(root, 'parse-bad', accept['old'], accept['new'], 'supports_claim')
    ledger['cases'].append(reject)
    L.validate(root, ledger)
    results = {'cases': [{'case_id': 'parse', 'status': 'passed'}, {'case_id': 'parse-bad', 'status': 'failed',
                                                                      'errors': ['accepted without diagnostics']}]}
    card = L.build_scorecard(ledger, json.loads((root / 'inventory.json').read_bytes()),
                             json.loads((root / 'ledger.json').read_bytes()), results)
    row = next(r for e in card['languages'] for r in e['rows'] if r['lane'] == 'parser_fidelity'
               and r['class'] == 'upstream_gold')
    assert (row['valid_acceptance_rate']['numerator'], row['valid_acceptance_rate']['denominator']) == (1, 1)
    assert (row['invalid_rejection_rate']['numerator'], row['invalid_rejection_rate']['denominator']) == (0, 1)
    assert row['preservation_false_alarm_rate']['value'] is None
    assert 'parser_fidelity' in row['preservation_false_alarm_rate']['na_reason']


# --- one parser-fidelity adapter for #262 (statement inventory) and #265 (accept/reject) ---

def _inventory_root(tmp_path, parse):
    root, ledger, case = _parser_root(tmp_path)
    case['expected']['parse'] = parse
    return root, ledger, case


def test_statement_inventory_case_validates_in_parser_lane(tmp_path):
    inventory = {'count': 1, 'starts': [0], 'evidence': 'reference engine'}
    for parse in ({'side': 'old', 'statement_inventory': inventory}, {'side': 'old', 'statement_inventory': None},
                  {'accepts': True, 'side': 'new', 'statement_inventory': inventory}):
        root, ledger, _ = _inventory_root(tmp_path / str(len(str(parse))), parse)
        L.validate(root, ledger)


@pytest.mark.parametrize('parse, message', [
    ({'statement_inventory': None}, 'side must be'),
    ({'accepts': True, 'side': 'both'}, 'side must be'),
    ({'accepts': False, 'side': 'old', 'statement_inventory': {'count': 1, 'starts': [0], 'evidence': 'e'}},
     'must-reject'),
])
def test_unified_parse_schema_rejects(tmp_path, parse, message):
    root, ledger, _ = _inventory_root(tmp_path, parse)
    with pytest.raises(L.LedgerError, match=message):
        L.validate(root, ledger)


def test_runner_combines_inventory_and_acceptance(tmp_path):
    R = _load('run_external_ledger', 'run_external_ledger.py')
    inventory = {'count': 1, 'starts': [0], 'evidence': 'e'}
    clean = {'status': 'passed', 'classification_errors': [], 'tree_errors': [], 'review_errors': [],
             'parse_errors': [], 'deterministic': True,
             'review': {'has_semantic_changes': False, 'is_style_only': True, 'is_fallback': False, 'changes': [],
                        'completeness': 'complete'},
             'trees': {'old': {'tree': T._node(0, 7, [T._node(0, 6)])}}}
    lost = dict(clean, trees={'old': {'tree': T._node(0, 7, [])}})
    fallback = {'status': 'failed', 'explicit_fallback': {'fallback': 'x'}}
    # #262 shape: the reference acceptance is a precondition; a fallback stays unsupported.
    root, _, case = _inventory_root(tmp_path / 'i', {'side': 'old', 'statement_inventory': inventory})
    assert R.classify(case, clean, root) == ('passed', [])
    status, errors = R.classify(case, lost, root)
    assert status == 'failed' and any('statement inventory' in e for e in errors)
    assert R.classify(case, fallback, root)[0] == 'unsupported'
    # Both claims: an explicit must-accept turns the fallback into a failure, and the inventory still applies.
    root, _, case = _inventory_root(tmp_path / 'b', {'accepts': True, 'side': 'old', 'statement_inventory': inventory})
    assert R.classify(case, clean, root) == ('passed', [])
    assert R.classify(case, lost, root)[0] == 'failed'
    assert R.classify(case, fallback, root)[0] == 'failed'


def test_inventory_case_scores_as_must_accept(tmp_path):
    root, ledger, case = _inventory_root(tmp_path, {'side': 'old', 'statement_inventory': None})
    L.validate(root, ledger)
    card = L.build_scorecard(ledger, json.loads((root / 'inventory.json').read_bytes()),
                             json.loads((root / 'ledger.json').read_bytes()),
                             {'cases': [{'case_id': 'parse', 'status': 'passed'}]})
    row = next(r for e in card['languages'] for r in e['rows'] if r['lane'] == 'parser_fidelity')
    assert (row['valid_acceptance_rate']['numerator'], row['valid_acceptance_rate']['denominator']) == (1, 1)
    assert row['invalid_rejection_rate']['denominator'] == 0
