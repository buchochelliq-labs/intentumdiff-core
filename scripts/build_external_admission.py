#!/usr/bin/env python3
"""Deterministically build the schema-2 admission ledger for the first #261 slice.

Benchmark tooling only. Inputs are the revision-pinned HumanEvalPack bytes already
retained under tests/fixtures/external/upstream (hash-checked against the importer's
pins), the pilot case bytes, independently derived controls written here, and the
executed witness records under admission/witnesses/. Admission is decided ONLY by
the witness verdict plus the source review recorded below; IntentumDiff output is
never consulted. Re-running with unchanged inputs reproduces the ledger byte-for-byte.

    python scripts/build_external_admission.py            # (re)write the ledger
    python scripts/build_external_admission.py --check    # fail if the ledger is stale
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures' / 'external'
_spec = importlib.util.spec_from_file_location('external_ledger', Path(__file__).with_name('external_ledger.py'))
ledger_lib = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(ledger_lib)
_spec = importlib.util.spec_from_file_location('hep_import', Path(__file__).with_name('import_external_benchmarks.py'))
hep_import = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(hep_import)
sha, encode = ledger_lib.sha, ledger_lib.encode

REVISION = hep_import.REVISION
CONFIGS = hep_import.LANGUAGES  # config -> (language_id, extension)
TASKS = range(12)
SUITE = 'humanevalpack'
LITERALS = 'intentumdiff-literal-regressions'
DIALECT = {'python': 'Python 3 (CPython 3.13 witness)', 'js': 'JavaScript (Node.js 22, CommonJS script)',
           'java': 'Java 21 (javac/java)', 'go': 'Go 1.24 (package main, upstream import field)',
           'cpp': 'C++17 (g++ -std=c++17)', 'rust': 'Rust 2021 edition (cargo; rand/regex/md5 per upstream imports)',
           'c': 'C11 (gcc -std=c11)'}
MIT_ATTRIBUTION = ('HumanEvalPack, BigCode project (OctoPack: Muennighoff et al., arXiv:2308.07124); '
                   'dataset card declares license: mit. Python split derives from OpenAI HumanEval.')
MODIFY_TYPES = ['MODIFICATION', 'ADDITION', 'DELETION']
ALL_TYPES = list(ledger_lib.CHANGE_TYPES)

# Source review of every admitted repair delta (independent of engine output): each
# entry was inspected as a minimal old/new byte region against the upstream bug_type.
# A task listed in REJECTED_REPAIRS stays a candidate with the recorded reason.
_GO_MATH = ('Witness inconclusive: the upstream Go `test` calls math.* but `test_setup` does not import math, '
            'so the test file does not compile (both sides). Needs a reviewed harness adapter; not admitted.')
_GO_HELPER = ('Witness inconclusive and case bytes incomplete: the IsPalindrome helper exists only in the upstream '
              '`prompt`, not in `declaration`, so declaration+solution does not compile. Re-import from prompt '
              'before admission; not admitted.')
REJECTED_REPAIRS: dict[str, str] = {
    'hep-go-02-repair': _GO_MATH, 'hep-go-02-identity': _GO_MATH,
    'hep-go-04-repair': _GO_MATH, 'hep-go-04-identity': _GO_MATH,
    'hep-go-10-repair': _GO_HELPER, 'hep-go-10-identity': _GO_HELPER,
}

# Local-identifier rename controls (behavior-preserving refactor): exactly these word
# tokens are renamed inside the canonical task-0 body. Each identifier is local to the
# function and has no other occurrence in the file (checked below).
LOCAL_RENAMES = {'python': ('distance', 'gap'), 'js': ('distance', 'gap'), 'java': ('distance', 'gap'),
                 'go': ('distance', 'gap'), 'cpp': ('j', 'k'), 'rust': ('distance', 'gap')}

MINIMAL_CHAR = ("char f(){return '0';}\n", "char f(){return '1';}\n")
CHAR_WITNESS = {'c': '#include <stdio.h>\nint main(void){ return f() == \'1\' ? 0 : 1; }\n',
                'cpp': '#include <cstdio>\nint main(){ return f() == \'1\' ? 0 : 1; }\n'}


def ref(path: str) -> dict:
    return {'path': path, 'sha256': sha((ROOT / path).read_bytes())}


CHECK_ONLY = False


def write(path: str, text: str) -> dict:
    target = ROOT / path
    data = text.encode()
    if not target.exists() or target.read_bytes() != data:
        if CHECK_ONLY:
            raise ValueError(f'derived case bytes are stale: {path}')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    return ref(path)


def minimal_region(old: bytes, new: bytes):
    """Independent byte delta: common prefix/suffix trimmed; inclusive-touch ranges."""
    prefix = 0
    while prefix < min(len(old), len(new)) and old[prefix] == new[prefix]:
        prefix += 1
    suffix = 0
    while suffix < min(len(old), len(new)) - prefix and old[-1 - suffix] == new[-1 - suffix]:
        suffix += 1
    return ({'start_byte': prefix, 'end_byte': len(old) - suffix},
            {'start_byte': prefix, 'end_byte': len(new) - suffix})


def witness_ref(case_id):
    path = f'admission/witnesses/{case_id}.json'
    if not (ROOT / path).exists():
        return None, None
    record = json.loads((ROOT / path).read_bytes())
    return ref(path), record


def classification(has_semantic, style, empty, completeness=None):
    return {'has_semantic_changes': has_semantic, 'is_style_only': style, 'changes_empty': empty,
            'completeness': completeness}


def forbid(kind, reason, change_type=None):
    item = {'kind': kind, 'reason': reason}
    if change_type:
        item['change_type'] = change_type
    return item


def external_case(*, case_id, config, task, old, new, family, claim, assumptions, derivation,
                  expected, review, plan, upstream_fields, known_issue=None):
    language, ext = CONFIGS[config]
    record = f'upstream/{config}/{task:02}.json'
    row = json.loads((ROOT / record).read_bytes())
    witness, verdict_record = witness_ref(case_id)
    verdict = verdict_record['verdict'] if verdict_record else None
    verbatim = derivation[0]['step'] == 'verbatim_upstream'
    admitted = verdict == 'supports_claim' and case_id not in REJECTED_REPAIRS
    if admitted:
        cls = 'upstream_gold' if verbatim else 'reviewed_external_derived'
        status, reason = 'admitted', review + f' Witness verdict: {verdict}.'
    else:
        cls, status = 'external_seed_only', 'candidate'
        reason = REJECTED_REPAIRS.get(case_id) or (
            f'Witness verdict {verdict}; not admitted.' if verdict else 'No executed witness yet; not admitted.')
    case = {
        'id': case_id, 'suite_id': SUITE, 'language_id': language, 'dialect': DIALECT[config],
        'filename': f'fixture.{ext}', 'class': cls, 'lane': 'change_understanding', 'change_family': family,
        'upstream': {'case_id': row['task_id'], 'row_index': task, 'fields': upstream_fields,
                     'bug_type': row.get('bug_type'), 'failure_symptoms': row.get('failure_symptoms'),
                     'record': ref(record)},
        'old': old, 'new': new, 'licence': 'MIT', 'attribution': MIT_ATTRIBUTION,
        'derivation': derivation,
        'behavior': {'claim': claim, 'assumptions': assumptions},
        'oracle': {'basis': 'executable_witness' if witness else 'candidate_only', 'witness': witness,
                   'byte_identity': old == new,
                   'notes': 'Upstream HumanEvalPack `test` executed against the exact case bytes by '
                            'scripts/humanevalpack_witness.py. Tests are bounded evidence for exercised inputs, '
                            'not program equivalence. IntentumDiff output is not an input to this oracle.'},
        'admission': {'status': status, 'reason': reason},
        'expected': expected if admitted else None,
        'witness_plan': {'config': config, 'upstream_record': ref(record), **plan},
    }
    if known_issue:
        case['known_issue'] = known_issue
    return case


def repair_cases(config):
    language, ext = CONFIGS[config]
    out = []
    for task in TASKS:
        base = f'cases/{config}/{task:02}'
        old, new = ref(f'{base}/old.{ext}'), ref(f'{base}/new.{ext}')
        old_b, new_b = (ROOT / old['path']).read_bytes(), (ROOT / new['path']).read_bytes()
        old_r, new_r = minimal_region(old_b, new_b)
        row = json.loads((ROOT / f'upstream/{config}/{task:02}.json').read_bytes())
        wrapper = ' Go: `package main` clause plus upstream `import` field prepended, as the upstream Go harness compiles them.' if config == 'go' else ''
        required = [{'kind': 'edit_region', 'change_types_any_of': MODIFY_TYPES, 'old_range': old_r, 'new_range': new_r,
                     'evidence': f'a reported edit must touch the independently computed delta bytes '
                                 f'{old_r["start_byte"]}..{old_r["end_byte"]} (old) / {new_r["start_byte"]}..{new_r["end_byte"]} (new)'}]
        known = None
        if config == 'cpp' and task == 11:
            required += [{'kind': 'change_fact', 'change_types_any_of': ['MODIFICATION'], 'match': 'descendant_evidence',
                          'old_label': a, 'new_label': b,
                          'evidence': f"output literal {a} -> {b}, inspected directly in the upstream pair"}
                         for a, b in (("'1'", "'0'"), ("'0'", "'1'"))]
            known = 'https://github.com/buchochelliq-labs/intentumdiff-cpp-parser/issues/5'
        expected = {'classification': classification(True, False, False), 'required': required, 'alternatives': [],
                    'forbidden': [forbid('style_only', 'a witnessed behavior-changing repair is not style-only'),
                                  forbid('no_changes', 'the delta must be reported'),
                                  forbid('fallback', 'valid source must be reviewed, not fall back')]}
        review = (f"Source delta reviewed: upstream {row['task_id']} buggy_solution -> canonical_solution "
                  f"(bug_type: {row.get('bug_type')}; failure_symptoms: {row.get('failure_symptoms')}); "
                  'region is executable code, not trivia.')
        out.append(external_case(
            case_id=f'hep-{config}-{task:02}-repair', config=config, task=task, old=old, new=new, family='repair',
            claim='changed', assumptions=f'Upstream test, {DIALECT[config]}; buggy side fails/times out, canonical passes.',
            derivation=[{'step': 'verbatim_upstream', 'description': 'declaration + buggy_solution (old) and declaration + '
                         'canonical_solution (new), UTF-8, unmodified.' + wrapper,
                         'tool': 'scripts/import_external_benchmarks.py'}],
            expected=expected, review=review, plan={}, upstream_fields=['declaration', 'buggy_solution', 'canonical_solution']
            + (['import'] if config == 'go' else []), known_issue=known))
        identity_expected = {'classification': classification(False, None, True), 'required': [], 'alternatives': [],
                             'forbidden': [forbid('change_type', 'byte-identical input has no edit', t) for t in ALL_TYPES]
                             + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
        out.append(external_case(
            case_id=f'hep-{config}-{task:02}-identity', config=config, task=task, old=new, new=new, family='identity',
            claim='preserved', assumptions='Old and new are the identical canonical bytes; witness confirms they pass the upstream test.',
            derivation=[{'step': 'identity_control', 'description': 'canonical program used as both sides'}],
            expected=identity_expected, review='Byte identity: identical path and SHA-256 on both sides. '
            'is_style_only is unconstrained: the public API does not define it for an empty edit.',
            plan={}, upstream_fields=['declaration', 'canonical_solution']))
    return out


def control_cases(config):
    language, ext = CONFIGS[config]
    row = json.loads((ROOT / f'upstream/{config}/00.json').read_bytes())
    prefix = '' if config == 'python' else f'{config}-'
    out = []
    # formatting (pilot bytes, reused)
    old, new = ref(f'cases/controls/{prefix}formatting-old.{ext}'), ref(f'cases/controls/{prefix}formatting-new.{ext}')
    out.append(external_case(
        case_id=f'hep-{config}-00-formatting', config=config, task=0, old=old, new=new, family='formatting',
        claim='preserved', assumptions='Only blank lines inserted between lexical tokens; same upstream test on both sides.',
        derivation=[{'step': 'derived_transformation', 'description': 'blank-line insertion into canonical task 0 '
                     '(Python: every newline doubled; others: one leading newline)', 'tool': 'scripts/import_external_benchmarks.py'}],
        expected={'classification': classification(False, True, None, 'equivalent'), 'required': [], 'alternatives': [],
                  'forbidden': [forbid('change_type', 'whitespace between tokens is not a code edit', t)
                                for t in ('ADDITION', 'DELETION', 'MODIFICATION', 'MOVE', 'REORDER')]
                  + [forbid('fallback', 'valid source must be reviewed, not fall back')]},
        review='Exact source delta reviewed: only newline bytes added.', plan={},
        upstream_fields=['declaration', 'canonical_solution']))
    # public function rename (pilot bytes, reused)
    entry = row['entry_point']
    replacement = 'nearbyElements' if config in ('js', 'java') else 'NearbyElements' if config == 'go' else 'nearby_elements'
    old, new = ref(f'cases/controls/{prefix}rename-old.{ext}'), ref(f'cases/controls/{prefix}rename-new.{ext}')
    out.append(external_case(
        case_id=f'hep-{config}-00-rename', config=config, task=0, old=old, new=new, family='rename_public',
        claim='preserved', assumptions='Computation preserved when callers follow the renamed public binding '
        '(witness: original test on old, test with the entry point renamed on new). The public API name changes, '
        'so this is a refactoring, not style.',
        derivation=[{'step': 'derived_transformation', 'description': f'single public binding {entry} -> {replacement}',
                     'tool': 'scripts/import_external_benchmarks.py'}],
        expected={'classification': classification(None, False, False), 'alternatives': [],
                  'required': [{'kind': 'change_fact', 'change_types_any_of': ['REFACTORING'], 'old_label': entry,
                                'new_label': replacement, 'evidence': f'public binding renamed {entry} -> {replacement}'}],
                  'forbidden': [forbid('change_type', 'a rename is not an add/delete of the entity', t)
                                for t in ('ADDITION', 'DELETION')]
                  + [forbid('style_only', 'renaming a public binding changes the API; not style-only')]},
        review='Exact source delta reviewed: one identifier token replaced; no internal reference to the old binding.',
        plan={'new_test_substitutions': [{'pattern': r'\b' + re.escape(entry) + r'\b', 'replacement': replacement}]},
        upstream_fields=['declaration', 'canonical_solution']))
    # local rename refactor (new derived bytes)
    original = (ROOT / f'cases/{config}/00/new.{ext}').read_text()
    name, renamed = LOCAL_RENAMES[config]
    pattern = r'\b' + re.escape(name) + r'\b'
    if re.search(r'\b' + re.escape(renamed) + r'\b', original) or len(re.findall(pattern, original)) < 2:
        raise ValueError(f'{config}: local rename precondition failed')
    changed = re.sub(pattern, renamed, original)
    old = write(f'admission/cases/controls/{config}-local-rename-old.{ext}', original)
    new = write(f'admission/cases/controls/{config}-local-rename-new.{ext}', changed)
    old_r, new_r = minimal_region(original.encode(), changed.encode())
    out.append(external_case(
        case_id=f'hep-{config}-00-local-rename', config=config, task=0, old=old, new=new, family='rename_local',
        claim='preserved', assumptions=f'Every occurrence of the function-local identifier `{name}` renamed to `{renamed}`; '
        'no other binding shares either name. Same upstream test on both sides.',
        derivation=[{'step': 'derived_transformation', 'description': f'local identifier rename {name} -> {renamed} in canonical task 0',
                     'tool': 'scripts/build_external_admission.py'}],
        expected={'classification': classification(None, False, False), 'alternatives': [],
                  'required': [{'kind': 'edit_region', 'change_types_any_of': ['REFACTORING', 'MODIFICATION'],
                                'old_range': old_r, 'new_range': new_r,
                                'evidence': f'the renamed local `{name}` -> `{renamed}` must be visible as a rename or modification'}],
                  'forbidden': [forbid('change_type', 'a local rename does not add or delete an entity', t)
                                for t in ('ADDITION', 'DELETION')]
                  + [forbid('style_only', 'an identifier rename is a refactoring, not formatting')]},
        review=f'Exact source delta reviewed: only `{name}` word tokens replaced.', plan={},
        upstream_fields=['declaration', 'canonical_solution']))
    return out


def literal_cases():
    out = []
    suite_files = []

    def hw(case_id, language, old, new, family, claim, expected, notes, plan=None, issue=None):
        ext = language
        suite_files.extend([old, new])
        case = {'id': case_id, 'suite_id': LITERALS, 'language_id': language, 'dialect': DIALECT[language],
                'filename': f'fixture.{ext}', 'class': 'handwritten_regression', 'lane': 'change_understanding',
                'change_family': family, 'upstream': {'case_id': case_id, 'fields': []},
                'old': old, 'new': new, 'licence': 'MIT', 'attribution': 'IntentumDiff contributors',
                'derivation': [{'step': 'handwritten', 'description': notes}],
                'behavior': {'claim': claim, 'assumptions': 'Freestanding translation unit; character literal value semantics.'},
                'oracle': {'basis': 'executable_witness' if plan else 'source_review', 'witness': None,
                           'byte_identity': old == new, 'notes': notes},
                'admission': {'status': 'admitted', 'reason': 'Handwritten regression; supplementary, excluded from external-gold denominators.'},
                'expected': expected}
        if plan:
            witness, _ = witness_ref(case_id)
            case['oracle']['witness'] = witness
            case['witness_plan'] = plan
        if issue:
            case['known_issue'] = issue
        out.append(case)

    changed = lambda a, b: {'classification': classification(True, False, False), 'alternatives': [],
                            'required': [{'kind': 'change_fact', 'change_types_any_of': ['MODIFICATION'], 'match': 'descendant_evidence',
                                          'old_label': a, 'new_label': b, 'evidence': f'character literal {a} -> {b}'}],
                            'forbidden': [forbid('style_only', 'a literal value change is not style-only'),
                                          forbid('no_changes', 'the literal edit must be reported')]}
    for language in ('c', 'cpp'):
        old = write(f'admission/cases/regression/{language}-minimal-char-old.{language}', MINIMAL_CHAR[0])
        new = write(f'admission/cases/regression/{language}-minimal-char-new.{language}', MINIMAL_CHAR[1])
        hw(f'literal-{language}-minimal-char', language, old, new, 'character_literal', 'changed', changed("'0'", "'1'"),
           "Minimised #261 pilot loss: char f(){return '0';} -> '1'.",
           plan={'config': language, 'inline_test': CHAR_WITNESS[language]},
           issue='https://github.com/buchochelliq-labs/intentumdiff-cpp-parser/issues/5')
        for name, a, b in (('ordinary', "'0'", "'1'"), ('escape', "'\\n'", "'\\t'"), ('position', "'a'", "'b'")):
            hw(f'literal-{language}-{name}', language, ref(f'cases/regression/{language}-{name}-old.{language}'),
               ref(f'cases/regression/{language}-{name}-new.{language}'), 'character_literal', 'changed', changed(a, b),
               'Pilot literal regression (core#261), reused bytes.')
    hw('literal-cpp-prefix', 'cpp', ref('cases/regression/cpp-prefix-old.cpp'), ref('cases/regression/cpp-prefix-new.cpp'),
       'character_literal', 'changed', changed("'x'", "L'x'"), 'Pilot literal regression: character type prefix added.')
    return out, suite_files


FAMILIES = Path(__file__).with_name('external_families')


def family_contributions():
    """Per-family case builders (#262-#267), each in its own module under scripts/external_families/.

    Each module exposes ``contribute(ctx)`` returning ``{'suites': [...], 'discovery': [...],
    'cases': [...]}``. Modules are merged in sorted file-name order, so independently added
    families rebuild one deterministic ledger.
    """
    from types import SimpleNamespace
    ctx = SimpleNamespace(ROOT=ROOT, ref=ref, write=write, minimal_region=minimal_region, witness_ref=witness_ref,
                          classification=classification, forbid=forbid, ledger_lib=ledger_lib, sha=sha)
    out = []
    for path in sorted(FAMILIES.glob('*.py')):
        spec = importlib.util.spec_from_file_location(f'external_family_{path.stem}', path)
        module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
        out.append(module.contribute(ctx))
    return out


def build():
    for config in CONFIGS:
        parquet = ROOT / f'upstream/{config}/test-00000-of-00001.parquet'
        if sha(parquet.read_bytes()) != hep_import.PARQUET_SHA256[config]:
            raise ValueError(f'pinned Parquet hash mismatch: {config}')
    cases = []
    for config in CONFIGS:
        cases += repair_cases(config) + control_cases(config)
    literal, literal_files = literal_cases()
    cases += literal
    discovery = []
    try:
        import pyarrow.parquet as pq
    except ImportError:
        pq = None
    for config, (language, _) in CONFIGS.items():
        parquet = ROOT / f'upstream/{config}/test-00000-of-00001.parquet'
        if pq is not None:
            rows = pq.read_table(parquet).to_pylist()
            discovered = len(rows)
            eligible = sum(bool(r['buggy_solution'] and r['canonical_solution'] and r['test']) for r in rows)
        else:  # recorded counts are re-derived whenever PyArrow is present (CI installs it)
            previous = json.loads((ROOT / 'admission/ledger.json').read_bytes())
            old_row = next(d for d in previous['discovery'] if d['language_id'] == language)
            discovered, eligible = old_row['discovered'], old_row['eligible']
        discovery.append({'suite_id': SUITE, 'language_id': language, 'lane': 'change_understanding',
                          'discovered': discovered, 'eligible': eligible,
                          'eligibility_rule': 'row has buggy_solution, canonical_solution and test',
                          'imported_rows': list(TASKS),
                          'holdout': 'rows 12-163 not imported; translated tasks overlap across languages, so '
                                     'portfolio totals must deduplicate by task index'})
    hep_files = [ref(f'upstream/{c}/test-00000-of-00001.parquet') for c in CONFIGS] + [ref('upstream/README.md')]
    suites = [
        {'suite_id': SUITE, 'name': 'HumanEvalPack (HumanEvalFix pairs + tests)', 'version': 'Hub dataset revision '
         + REVISION[:12] + '; OctoPack arXiv:2308.07124', 'url': 'https://huggingface.co/datasets/bigcode/humanevalpack',
         'revision': REVISION, 'licence': 'MIT', 'licence_evidence': 'dataset card front matter `license: mit` '
         '(upstream/README.md); no separate LICENSE file exists at this revision (MCP listing, 2026-11-17)',
         'attribution': MIT_ATTRIBUTION, 'evidence_class': 'published specialist research benchmark',
         'acquisition': {
             'method': 'revision-pinned Parquet bytes retained in-repository since the 2026-11-08 foundation '
                       '(import_external_benchmarks.PARQUET_SHA256)',
             'files': hep_files,
             'verification': [
                 'Hugging Face MCP hf_fs ls @' + REVISION + ' (2026-11-17): six */test-00000-of-00001.parquet LFS files, '
                 'README.md, .gitattributes; no other files.',
                 'MCP stat byte sizes equal the retained Parquet sizes for all six configs (cpp 193981, go 198394, '
                 'java 210440, js 194044, python 191279, rust 168464) and README.md (12521).',
                 'MCP cat of README.md at the pinned revision: dataset_info download_size equals each retained Parquet size.',
                 'BLOCKED: MCP hf_fs refuses binary cat (HF_FS_TEXT_ONLY) and exposes no LFS sha256, and huggingface.co is '
                 'not reachable from this sandbox, so the Parquet SHA-256 values could not be re-derived from the Hub '
                 'on 2026-11-17; they rest on the 2026-11-08 download.',
                 'Dataset viewer preview timed out; no viewer/Parquet row cross-comparison is claimed.']}},
        {'suite_id': LITERALS, 'name': 'IntentumDiff C/C++ character-literal regressions', 'version': 'v2',
         'url': 'https://github.com/buchochelliq-labs/intentumdiff-core/issues/261', 'revision': 'in-repository',
         'handwritten': True, 'licence': 'MIT', 'attribution': 'IntentumDiff contributors',
         'evidence_class': 'handwritten regression (supplementary)',
         'acquisition': {'method': 'authored in repository', 'files': literal_files,
                         'verification': ['bytes pinned per file by SHA-256']}},
    ]
    for family in family_contributions():
        suites += family['suites']; discovery += family['discovery']; cases += family['cases']
    ledger = {'schema_version': 2, 'contract': 'core#268 admission ledger; parent core#261',
              'inventory': ref('inventory.json'), 'language_selection': ref('ledger.json'),
              'suites': suites, 'discovery': discovery, 'cases': cases}
    return ledger


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    global CHECK_ONLY
    CHECK_ONLY = args.check
    try:
        ledger = build()
    except ValueError as error:
        print(f'admission build failed: {error}', file=sys.stderr)
        return 1
    data = encode(ledger)
    target = ROOT / 'admission' / 'ledger.json'
    if args.check:
        if not target.exists() or target.read_bytes() != data:
            print('admission ledger is stale; re-run scripts/build_external_admission.py', file=sys.stderr)
            return 1
        return 0
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    ledger_lib.validate(ROOT)
    counts = {}
    for c in ledger['cases']:
        counts[(c['language_id'], c['class'], c['admission']['status'])] = counts.get((c['language_id'], c['class'], c['admission']['status']), 0) + 1
    for key in sorted(counts):
        print(*key, counts[key])
    return 0


if __name__ == '__main__':
    sys.exit(main())
