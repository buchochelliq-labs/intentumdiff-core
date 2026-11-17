"""Schema-2 admission ledger, judge and scorecard contract tests (core #268)."""
import copy
import importlib.util
import json
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).resolve().parents[2] / 'scripts'
spec = importlib.util.spec_from_file_location('external_ledger', SCRIPTS / 'external_ledger.py')
L = importlib.util.module_from_spec(spec); spec.loader.exec_module(L)
REAL = SCRIPTS.parent / 'tests' / 'fixtures' / 'external'
REV = 'a' * 40


def _ref(root, path, data):
    (root / path).parent.mkdir(parents=True, exist_ok=True)
    (root / path).write_bytes(data)
    return {'path': path, 'sha256': L.sha(data)}


def _witness(root, case_id, old, new, verdict):
    record = {'case_id': case_id, 'old_sha256': old['sha256'], 'new_sha256': new['sha256'], 'verdict': verdict}
    return _ref(root, f'admission/witnesses/{case_id}.json', json.dumps(record).encode())


def _expected(semantic=True):
    return {'classification': {'has_semantic_changes': semantic, 'is_style_only': not semantic and None,
                               'changes_empty': not semantic, 'completeness': None},
            'required': ([{'kind': 'edit_region', 'change_types_any_of': ['MODIFICATION'],
                           'old_range': {'start_byte': 16, 'end_byte': 19}, 'new_range': {'start_byte': 16, 'end_byte': 19},
                           'evidence': 'literal'}] if semantic else []),
            'alternatives': [],
            'forbidden': [{'kind': 'style_only', 'reason': 'r'}] if semantic else
                         [{'kind': 'change_type', 'change_type': t, 'reason': 'identity'} for t in L.CHANGE_TYPES]}


def make_root(tmp_path, verdict='supports_claim'):
    root = tmp_path
    inventory = {'schema_version': 1, 'extension_index': {}, 'parsers': {
        'c': {'wasm': 'cpp_parser.wasm'}, 'cpp': {'wasm': 'cpp_parser.wasm'}, 'zig': {'wasm': 'zig_parser.wasm'}}}
    inv = _ref(root, 'inventory.json', json.dumps(inventory).encode())
    selection = {'schema_version': 1, 'languages': [
        {'language_id': lang, 'source_url': 'https://example.org/' + lang, 'evidence_class': 'x',
         'reason': f'{lang} acquisition pending'} for lang in inventory['parsers']]}
    sel = _ref(root, 'ledger.json', json.dumps(selection).encode())
    upstream = _ref(root, 'upstream/task.json', b'{"task_id": "CPP/1", "test": "int main(){}"}')
    old = _ref(root, 'cases/old.cpp', b"char f(){return '0';}\n")
    new = _ref(root, 'cases/new.cpp', b"char f(){return '1';}\n")
    base = {'suite_id': 'hep', 'language_id': 'cpp', 'dialect': 'C++17', 'filename': 'f.cpp', 'lane': 'change_understanding',
            'licence': 'MIT', 'attribution': 'upstream authors', 'upstream': {'case_id': 'CPP/1', 'record': upstream},
            'admission': {'status': 'admitted', 'reason': 'reviewed'}}
    repair = dict(copy.deepcopy(base), id='repair', **{'class': 'upstream_gold'}, change_family='repair', old=old, new=new,
                  derivation=[{'step': 'verbatim_upstream'}], behavior={'claim': 'changed', 'assumptions': 'tests'},
                  oracle={'basis': 'executable_witness', 'notes': 'n',
                          'witness': _witness(root, 'repair', old, new, verdict)}, expected=_expected())
    identity = dict(copy.deepcopy(base), id='identity', **{'class': 'reviewed_external_derived'}, change_family='identity',
                    old=new, new=new, derivation=[{'step': 'identity_control'}],
                    behavior={'claim': 'preserved', 'assumptions': 'identical'},
                    oracle={'basis': 'executable_witness', 'notes': 'n', 'byte_identity': True,
                            'witness': _witness(root, 'identity', new, new, 'supports_claim')},
                    expected=_expected(False))
    seed = dict(copy.deepcopy(base), id='seed', **{'class': 'external_seed_only'}, change_family='repair', old=old, new=new,
                derivation=[{'step': 'verbatim_upstream'}], behavior={'claim': 'unknown', 'assumptions': 'none'},
                oracle={'basis': 'candidate_only', 'notes': 'n', 'witness': None}, expected=None,
                admission={'status': 'candidate', 'reason': 'not reviewed'})
    suite = {'suite_id': 'hep', 'name': 'HEP', 'version': '1', 'url': 'https://example.org', 'revision': REV,
             'licence': 'MIT', 'attribution': 'authors', 'evidence_class': 'research',
             'acquisition': {'method': 'test', 'files': [upstream], 'verification': []}}
    ledger = {'schema_version': 2, 'inventory': inv, 'language_selection': sel, 'suites': [suite],
              'discovery': [{'suite_id': 'hep', 'language_id': 'cpp', 'lane': 'change_understanding',
                             'discovered': 164, 'eligible': 160, 'eligibility_rule': 'has tests'}],
              'cases': [repair, identity, seed]}
    (root / 'admission').mkdir(exist_ok=True)
    (root / 'admission' / 'ledger.json').write_bytes(L.encode(ledger))
    return root, ledger


def test_checked_in_ledger_is_valid_and_reproducible():
    ledger, inventory = L.validate(REAL)
    assert {c['class'] for c in ledger['cases']} <= set(L.CLASSES)
    assert set(json.loads((REAL / 'ledger.json').read_bytes())['languages'][0]) >= {'language_id'}
    assert len(inventory['parsers']) == 74
    spec = importlib.util.spec_from_file_location('builder', SCRIPTS / 'build_external_admission.py')
    builder = importlib.util.module_from_spec(spec); spec.loader.exec_module(builder)
    builder.CHECK_ONLY = True  # never rewrite fixtures from a test
    assert L.encode(builder.build()) == (REAL / 'admission' / 'ledger.json').read_bytes(), 'ledger is stale'


def test_synthetic_ledger_validates(tmp_path):
    root, _ = make_root(tmp_path)
    L.validate(root)


@pytest.mark.parametrize('mutate, message', [
    (lambda l: l['cases'][0]['old'].update(path='../escape'), 'unsafe path'),
    (lambda l: l['cases'][0]['old'].update(sha256='0' * 64), 'sha256 mismatch'),
    (lambda l: l['cases'][2]['admission'].update(status='admitted'), 'seed-only'),
    (lambda l: l['cases'][0]['oracle'].update(witness=None), 'requires an executed witness'),
    (lambda l: l['cases'][1]['oracle'].update(basis='byte_identity'), 'requires an executed witness'),
    (lambda l: l['cases'][0].update(derivation=[{'step': 'derived_transformation'}]), 'verbatim upstream'),
    (lambda l: l['cases'][2].update(expected=_expected()), 'must not carry expectations'),
    (lambda l: l['cases'][0].update(**{'class': 'bogus'}), 'unknown class'),
    (lambda l: l['cases'][0].update(lane='vibes'), 'unknown lane'),
    (lambda l: l['cases'][0]['behavior'].update(claim='unknown'), 'changed/preserved'),
    (lambda l: l['cases'][0].pop('licence'), 'licence'),
    (lambda l: l['suites'][0].update(revision='main'), 'immutable'),
    (lambda l: l['cases'][0]['expected']['classification'].pop('is_style_only'), 'must be present'),
    (lambda l: l['cases'][0]['expected']['required'][0].update(change_types_any_of=[]), 'change_types_any_of'),
    (lambda l: l['cases'][2].update(id='repair'), 'duplicate case'),
    (lambda l: l['discovery'][0].update(eligible=999), 'eligible exceeds'),
    (lambda l: l['cases'][0].update(language_id='cobol'), 'unregistered language'),
])
def test_validator_rejects(tmp_path, mutate, message):
    root, ledger = make_root(tmp_path)
    mutate(ledger)
    with pytest.raises(L.LedgerError, match=message):
        L.validate(root, ledger)


def test_contradicting_witness_cannot_admit(tmp_path):
    root, ledger = make_root(tmp_path, verdict='contradicts_claim')
    with pytest.raises(L.LedgerError, match='supporting the claim'):
        L.validate(root, ledger)


def _review(changes, semantic, style):
    return {'has_semantic_changes': semantic, 'is_style_only': style, 'is_fallback': False, 'changes': changes}


LIT = {'change_type': 'MODIFICATION',
       'old_node': {'label': "'0'", 'position': {'start_line': 0, 'start_col': 16, 'end_line': 0, 'end_col': 19}},
       'new_node': {'label': "'1'", 'position': {'start_line': 0, 'start_col': 16, 'end_line': 0, 'end_col': 19}}}


def test_judge_rejects_constant_classifiers(tmp_path):
    _, ledger = make_root(tmp_path)
    repair, identity = ledger['cases'][0], ledger['cases'][1]
    old, new = b"char f(){return '0';}\n", b"char f(){return '1';}\n"
    assert L.judge(repair, _review([LIT], True, False), old, new) == []
    always_style = _review([], False, True)
    always_semantic = _review([dict(LIT)], True, False)
    assert L.judge(repair, always_style, old, new)
    assert L.judge(identity, always_semantic, new, new)
    assert L.judge(identity, _review([], False, None), new, new) == []
    # A change somewhere else in the file does not satisfy the edit-region evidence.
    elsewhere = copy.deepcopy(LIT)
    for side in ('old_node', 'new_node'):
        elsewhere[side]['position'] = {'start_line': 0, 'start_col': 0, 'end_line': 0, 'end_col': 4}
    assert any('missing required' in e for e in L.judge(repair, _review([elsewhere], True, False), old, new))
    # Wrong change type does not satisfy it either.
    moved = dict(copy.deepcopy(LIT), change_type='MOVE')
    assert any('missing required' in e for e in L.judge(repair, _review([moved], True, False), old, new))


def test_alternative_result_set_is_accepted(tmp_path):
    _, ledger = make_root(tmp_path)
    repair = copy.deepcopy(ledger['cases'][0])
    repair['expected']['required'][0]['change_types_any_of'] = ['REFACTORING']
    old, new = b"char f(){return '0';}\n", b"char f(){return '1';}\n"
    assert L.judge(repair, _review([LIT], True, False), old, new)
    repair['expected']['alternatives'] = [{'reason': 'modification is equally valid',
                                           'required': [dict(repair['expected']['required'][0], change_types_any_of=['MODIFICATION'])]}]
    assert L.judge(repair, _review([LIT], True, False), old, new) == []


def _card(tmp_path, statuses):
    root, ledger = make_root(tmp_path)
    ledger, inventory = L.validate(root)
    selection = json.loads((root / 'ledger.json').read_bytes())
    results = None if statuses is None else {'cases': [
        {'case_id': cid, 'status': st, 'errors': [] if st == 'passed' else ['x']} for cid, st in statuses.items()]}
    card = L.build_scorecard(ledger, inventory, selection, results)
    return {e['language_id']: e for e in card['languages']}, card


def test_scorecard_every_language_has_a_row_and_zero_case_is_na_gap(tmp_path):
    rows, card = _card(tmp_path, {'repair': 'passed', 'identity': 'passed'})
    assert set(rows) == {'c', 'cpp', 'zig'}
    for lang in ('c', 'zig'):
        for lane in L.LANES:
            entry = rows[lang]['lanes'][lane]
            assert entry['status'] == 'gap'
            rate = entry['external_gold_pass_rate']
            assert rate['value'] is None and rate['denominator'] == 0 and rate['na_reason']
            assert 'acquisition pending' in rate['na_reason']
    assert card['summary']['languages_without_measured_external_lane'] == 2
    lane = rows['cpp']['lanes']['change_understanding']
    assert lane['status'] == 'measured'
    assert lane['external_gold_pass_rate'] == {'value': 1.0, 'numerator': 2, 'denominator': 2}
    assert rows['cpp']['lanes']['parser_fidelity']['status'] == 'gap'


def test_scorecard_counts_and_denominators(tmp_path):
    rows, _ = _card(tmp_path, {'repair': 'failed', 'identity': 'passed'})
    by_class = {r['class']: r for r in rows['cpp']['rows']}
    gold = by_class['upstream_gold']
    assert gold['counts'] == {'discovered': 164, 'eligible': 160, 'admitted': 1, 'executed': 1, 'passed': 0,
                              'failed': 1, 'unsupported': 0, 'skipped': 0, 'blocked': 0}
    assert gold['meaningful_change_recall'] == {'value': 0.0, 'numerator': 0, 'denominator': 1}
    assert gold['preservation_false_alarm_rate']['value'] is None
    assert gold['failures'][0]['case_id'] == 'repair'
    derived = by_class['reviewed_external_derived']
    assert derived['preservation_false_alarm_rate'] == {'value': 0.0, 'numerator': 0, 'denominator': 1}
    seed = by_class['external_seed_only']
    assert seed['counts']['admitted'] == 0 and seed['pass_rate']['value'] is None


def test_scorecard_without_results_reports_skipped_not_passed(tmp_path):
    rows, _ = _card(tmp_path, None)
    gold = next(r for r in rows['cpp']['rows'] if r['class'] == 'upstream_gold')
    assert gold['counts']['skipped'] == 1 and gold['counts']['executed'] == 0
    assert gold['pass_rate']['value'] is None
    assert rows['cpp']['lanes']['change_understanding']['status'] == 'gap'


def test_scorecard_blocked_and_unsupported_are_not_passes(tmp_path):
    rows, _ = _card(tmp_path, {'repair': 'blocked', 'identity': 'unsupported'})
    lane = rows['cpp']['lanes']['change_understanding']
    assert lane['status'] == 'blocked' and lane['external_gold_pass_rate']['value'] is None
    counts = {r['class']: r['counts'] for r in rows['cpp']['rows']}
    assert counts['upstream_gold']['blocked'] == 1 and counts['reviewed_external_derived']['unsupported'] == 1


def test_scorecard_rejects_unknown_result_rows(tmp_path):
    with pytest.raises(L.LedgerError, match='unknown cases'):
        _card(tmp_path, {'ghost': 'passed'})


def test_markdown_never_shows_percent_for_zero_cases(tmp_path):
    _, card = _card(tmp_path, {'repair': 'passed', 'identity': 'passed'})
    text = L.render_markdown(card, ['# t'])
    zig = next(line for line in text.splitlines() if line.startswith('| `zig`'))
    assert '%' not in zig and 'GAP' in zig


def _runner():
    spec = importlib.util.spec_from_file_location('run_external_ledger', SCRIPTS / 'run_external_ledger.py')
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


def test_runner_maps_assess_rows_to_statuses(tmp_path):
    R = _runner()
    root, ledger = make_root(tmp_path)
    repair = ledger['cases'][0]
    good = {'status': 'passed', 'classification_errors': [], 'tree_errors': [], 'review_errors': [],
            'parse_errors': [], 'deterministic': True, 'review': _review([LIT], True, False)}
    assert R.classify(repair, good, root) == ('passed', [])
    # assess_languages accepted it, but the independent ledger oracle did not.
    style = dict(good, review=_review([], False, True))
    status, errors = R.classify(repair, style, root)
    assert status == 'failed' and any('missing required' in e for e in errors)
    assert R.classify(repair, {'status': 'not_executed', 'error': 'timeout'}, root)[0] == 'blocked'
    assert R.classify(repair, {'status': 'failed', 'explicit_fallback': {'fallback': 'x'}}, root)[0] == 'unsupported'
    assert R.classify(repair, dict(good, deterministic=False), root)[0] == 'failed'
    probe = R.to_probe(repair, root)
    assert probe['expected'] == {'has_semantic_changes': True, 'is_style_only': False, 'changes_empty': False,
                                 'completeness': None}


def test_runner_gate_regressions_only_for_previous_upstream_gold_passes():
    R = _runner()
    row = lambda cid, cls, st, issue=None: {'case_id': cid, 'class': cls, 'status': st, 'errors': ['e'], 'known_issue': issue}
    baseline = {'cases': [row('g1', 'upstream_gold', 'passed'), row('g2', 'upstream_gold', 'failed'),
                          row('d1', 'reviewed_external_derived', 'passed')]}
    ok = [row('g1', 'upstream_gold', 'passed'), row('g2', 'upstream_gold', 'failed'),
          row('d1', 'reviewed_external_derived', 'failed')]
    assert R.gate_failures(ok, baseline) == []          # known failure stays visible but does not gate
    regressed = [row('g1', 'upstream_gold', 'failed'), row('g2', 'upstream_gold', 'passed')]
    assert [g[0] for g in R.gate_failures(regressed, baseline)] == ['g1']
    assert [g[1] for g in R.gate_failures([row('g2', 'upstream_gold', 'failed')], baseline)] == ['missing']
    assert R.gate_failures([row('g2', 'upstream_gold', 'failed')], baseline, narrowed=True) == []
    blocked = ok + [row('b', 'handwritten_regression', 'blocked')]
    assert [g[0] for g in R.gate_failures(blocked, baseline)] == ['b']
    # Without a baseline, any upstream_gold failure without a linked issue gates.
    assert [g[0] for g in R.gate_failures(ok)] == ['g2']
    assert R.gate_failures([row('g2', 'upstream_gold', 'failed', 'https://x/1')]) == []
    assert {g[0] for g in R.gate_failures(ok, strict=True)} == {'g2', 'd1'}
