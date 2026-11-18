"""#267 build/configuration/infrastructure family: ledger contribution, witnesses and extraction helpers."""
import importlib.util
import json
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[2] / 'scripts'
REAL = SCRIPTS.parent / 'tests' / 'fixtures' / 'external'
BUILD_INFRA = ('dockerfile', 'hcl', 'gomod', 'gitignore', 'cmake', 'make', 'puppet')


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


L = _load('external_ledger', SCRIPTS / 'external_ledger.py')
F = _load('family_267', SCRIPTS / 'external_families' / 'family_267_build_infra.py')


def _family_cases(ledger):
    suites = set(F.SUITES)
    return [c for c in ledger['cases'] if c['suite_id'] in suites]


def test_family_cases_are_merged_and_witnessed():
    ledger, _ = L.validate(REAL)
    cases = _family_cases(ledger)
    assert {c['language_id'] for c in cases} == set(BUILD_INFRA)
    for case in cases:
        if case['admission']['status'] != 'admitted':
            assert case['expected'] is None
            continue
        record = json.loads((REAL / case['oracle']['witness']['path']).read_bytes())
        assert record['verdict'] == 'supports_claim'
        assert (record['old_sha256'], record['new_sha256']) == (case['old']['sha256'], case['new']['sha256'])
        if case['lane'] == 'parser_fidelity':
            assert case['old'] == case['new'] and 'accepts' in case['expected']['parse']
        else:
            assert case['class'] in ('upstream_gold', 'reviewed_external_derived')


def test_each_language_has_changed_and_preserved_witnessed_pairs():
    ledger, _ = L.validate(REAL)
    claims = {}
    for case in _family_cases(ledger):
        if case['lane'] == 'change_understanding' and case['admission']['status'] == 'admitted':
            claims.setdefault(case['language_id'], set()).add(case['behavior']['claim'])
    assert all(claims.get(language) == {'changed', 'preserved'} for language in BUILD_INFRA), claims


def test_blocked_sources_are_recorded_and_scripting_ids_left_to_266():
    ledger, inventory = L.validate(REAL)
    blocked = {(b['suite_id'], b['language_id']) for b in ledger.get('blocked_acquisitions', [])}
    assert {('iac-eval', 'hcl'), ('shipwright', 'dockerfile')} <= blocked
    assert not [c for c in _family_cases(ledger) if c['language_id'] in ('bash', 'powershell')]


def test_txtar_and_heredoc_extraction():
    archive = b'comment\n-- .mod --\nmodule x\n\n-- .info --\n{}\n-- go.mod --\nmodule y\n'
    assert F.txtar_members(archive) == [('.mod', b'module x\n\n'), ('.info', b'{}\n'), ('go.mod', b'module y\n')]
    script = "\tcat <<-\\EOF >.gitignore &&\n\t\tone\n\t\t# c\n\n\tEOF\n\tsed -e \"s/Z$//\" >ignore <<-\\EOF &&\n\ta \\ Z\n\tEOF\n"
    docs = F.heredocs(script)
    assert [(d['target'], d['payload']) for d in docs] == [('.gitignore', 'one\n# c\n\n'), ('ignore', 'a \\ \n')]
