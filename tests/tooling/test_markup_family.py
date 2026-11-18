"""#263 web/markup family: fragment reproducibility, admission rules and blocked acquisitions."""
import importlib.util
import json
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[2] / 'scripts'
REAL = SCRIPTS.parent / 'tests' / 'fixtures' / 'external'
MARKUP = ('markdown', 'mdx', 'html', 'css', 'scss', 'vue', 'svelte', 'astro', 'latex', 'asciidoc', 'po')


def _load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


L = _load('external_ledger', 'external_ledger.py')


def test_checked_in_fragment_is_reproducible_and_merged():
    family = _load('markup_family', 'markup_family.py')
    family.CHECK_ONLY = True  # never rewrite fixtures from a test
    assert L.encode(family.build()) == family.FRAGMENT.read_bytes(), 'markup fragment is stale'
    ledger, _ = L.validate(REAL)
    fragment = json.loads(family.FRAGMENT.read_bytes())
    assert {c['id'] for c in fragment['cases']} <= {c['id'] for c in ledger['cases']}
    assert {c['language_id'] for c in fragment['cases']} == set(MARKUP)
    for case in fragment['cases']:
        if case['admission']['status'] == 'admitted':
            assert case['oracle']['witness'] is not None
            assert case['class'] in ('upstream_gold', 'reviewed_external_derived')
        else:
            assert case['expected'] is None
        if case['lane'] == 'parser_fidelity':
            assert case['old'] == case['new']


def test_commonmark_examples_are_extracted_by_the_spec_rule():
    family = _load('markup_family', 'markup_family.py')
    text = (REAL / 'upstream/markup/commonmark-spec/package/spec.txt').read_text(encoding='utf-8')
    examples = family.spec_examples(text)
    assert len(examples) == 652 and [e['number'] for e in examples] == list(range(1, 653))
    assert examples[0]['markdown'] == '\tfoo\tbaz\t\tbim\n'  # → is a tab


def test_inventory_only_where_every_block_line_is_covered():
    family = _load('markup_family', 'markup_family.py')
    blocks = [{'start_line': 1, 'end_line': 1}, {'start_line': 3, 'end_line': 3}]
    assert family._blocks_inventory(b'# a\n\nb\n', blocks)['starts'] == [0, 5]
    # a consumed link reference definition has no AST block: no inventory claim
    assert family._blocks_inventory(b'[x]: /u\n\nb\n', [{'start_line': 3, 'end_line': 3}]) is None


def test_postscript_and_generic_are_blocked_never_measured():
    ledger, inventory = L.validate(REAL)
    selection = json.loads((REAL / 'ledger.json').read_bytes())
    card = L.build_scorecard(ledger, inventory, selection, None)
    by_id = {e['language_id']: e for e in card['languages']}
    for language in ('postscript', 'generic'):
        for lane in ('parser_fidelity', 'change_understanding'):
            assert by_id[language]['lanes'][lane]['status'] == 'blocked'
            assert by_id[language]['lanes'][lane]['external_gold_pass_rate']['value'] is None
