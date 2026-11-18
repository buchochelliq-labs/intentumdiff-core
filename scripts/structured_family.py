#!/usr/bin/env python3
"""#265 structured/data/schema family for the schema-2 admission ledger (core #268).

Benchmark tooling only. Covers the registered IDs yaml, json, toml, xml, ini, proto and
graphql. Inputs are the revision-pinned upstream bytes acquired by
``scripts/acquire_structured_benchmarks.py`` under
``tests/fixtures/external/upstream/structured/``. Two lanes are kept apart:

* ``parser_fidelity``: single-source conformance. Each case is the verbatim upstream
  document on both sides (old == new); the oracle is the upstream accept/reject label,
  confirmed by an executed reference parser. ``expected.parse.accepts`` carries it.
* ``change_understanding``: upstream before/after pairs where they exist (buf breaking
  fixtures, graphql-js findBreakingChanges pairs, YAML Test Suite in.yaml -> out.yaml,
  JSON Patch doc -> expected), otherwise ``reviewed_external_derived`` edits of upstream
  seeds. Each claim (changed / preserved) is established by executing a reference
  implementation (typed value equality, schema/descriptor equality, buf breaking,
  graphql-js findBreakingChanges), never by IntentumDiff output.

Commands::

    python scripts/structured_family.py witness [--suite S ...]   # run reference oracles
    python scripts/structured_family.py build [--check]           # (re)write the fragment

The fragment ``tests/fixtures/external/admission/families/structured-265.json`` is
merged by ``scripts/build_external_admission.py``. Witness execution needs the
reference toolchains named in ``TOOLS``; ``build`` is offline and deterministic.
"""
from __future__ import annotations

import argparse
from collections import Counter
import configparser
from datetime import datetime, timezone
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures' / 'external'
UP = 'upstream/structured'
DERIVED = 'admission/families/structured-265'
WITNESS_DIR = 'admission/witnesses/structured'
FRAGMENT = ROOT / 'admission' / 'families' / 'structured-265.json'
_spec = importlib.util.spec_from_file_location('external_ledger', Path(__file__).with_name('external_ledger.py'))
L = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(L)
sha, encode = L.sha, L.encode

# Reference toolchains (witness only). Paths come from the environment so the witness
# can run anywhere; versions are recorded in every witness record.
TOOLS = {
    'python': sys.executable,
    'ruamel_python': os.environ.get('STRUCTURED_REF_PYTHON', sys.executable),  # needs ruamel.yaml + jsonpatch
    'node_modules': os.environ.get('STRUCTURED_REF_NODE_MODULES', ''),           # graphql@16.15.0, @bufbuild/buf@1.75.0
    'cc': os.environ.get('CC', 'gcc'),
}

ALL_TYPES = list(L.CHANGE_TYPES)
EDIT_TYPES = ['MODIFICATION', 'ADDITION', 'DELETION']
STRUCT_TYPES = ['MODIFICATION', 'ADDITION', 'DELETION', 'MOVE', 'REORDER']
DERIVED_LIMIT = 24  # per derivation family and suite: bounded, deterministic (sorted seeds)


def acquisition(suite):
    return json.loads((ROOT / UP / suite / 'ACQUISITION.json').read_bytes())


def upref(suite, path):
    rel = f'{UP}/{suite}/{path}'
    return {'path': rel, 'sha256': sha((ROOT / rel).read_bytes())}


def upbytes(suite, path):
    return (ROOT / UP / suite / path).read_bytes()


CHECK_ONLY = False


def write(rel, data: bytes):
    target = ROOT / rel
    if not target.exists() or target.read_bytes() != data:
        if CHECK_ONLY:
            raise ValueError(f'derived case bytes are stale: {rel}')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    return {'path': rel, 'sha256': sha(data)}


def utf8(data):
    try:
        data.decode('utf-8')
        return True
    except UnicodeDecodeError:
        return False


def minimal_region(old: bytes, new: bytes):
    prefix = 0
    while prefix < min(len(old), len(new)) and old[prefix] == new[prefix]:
        prefix += 1
    suffix = 0
    while suffix < min(len(old), len(new)) - prefix and old[-1 - suffix] == new[-1 - suffix]:
        suffix += 1
    return ({'start_byte': prefix, 'end_byte': len(old) - suffix},
            {'start_byte': prefix, 'end_byte': len(new) - suffix})


def classification(has_semantic, style, empty, completeness=None):
    return {'has_semantic_changes': has_semantic, 'is_style_only': style, 'changes_empty': empty,
            'completeness': completeness}


def forbid(kind, reason, change_type=None):
    item = {'kind': kind, 'reason': reason}
    if change_type:
        item['change_type'] = change_type
    return item


NO_FALLBACK = forbid('fallback', 'valid source must be reviewed, not fall back')


def expect_accept():
    return {'classification': classification(False, None, True), 'required': [], 'alternatives': [],
            'parse': {'accepts': True},
            'forbidden': [forbid('change_type', 'identical document', t) for t in ALL_TYPES]
            + [NO_FALLBACK]}


def expect_reject():
    return {'classification': classification(None, None, None), 'required': [], 'alternatives': [],
            'parse': {'accepts': False}, 'forbidden': []}


def expect_changed(old, new, types, evidence):
    o, n = minimal_region(old, new)
    return {'classification': classification(True, False, False), 'alternatives': [],
            'required': [{'kind': 'edit_region', 'change_types_any_of': types, 'old_range': o, 'new_range': n,
                          'evidence': f'{evidence}; a reported change must touch the independently computed delta '
                                      f'{o["start_byte"]}..{o["end_byte"]} (old) / {n["start_byte"]}..{n["end_byte"]} (new)'}],
            'forbidden': [forbid('style_only', 'a reference-witnessed value/schema change is not style-only'),
                          forbid('no_changes', 'the witnessed change must be reported'), NO_FALLBACK]}


def expect_preserved(forbidden_types, reason):
    return {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
            'forbidden': [forbid('change_type', reason, t) for t in forbidden_types] + [NO_FALLBACK]}


# --- suite metadata ----------------------------------------------------------------------

LIC = {
    'yaml-test-suite': ('MIT', 'License file at main commit da267a5c4782e7361e82889e76c0dc7df0e1e870 (the data branch is '
                        'generated from main and carries no licence file of its own)',
                        'YAML Test Suite, Ingy döt Net, Tina Müller and contributors (github.com/yaml/yaml-test-suite)'),
    'jsontestsuite': ('MIT', 'LICENSE at the pinned commit', 'JSONTestSuite, Nicolas Seriot (github.com/nst/JSONTestSuite)'),
    'json-patch-tests': ('Apache-2.0', 'README.md "License" section at the pinned commit (no LICENSE file)',
                         'JSON Patch Tests, The Authors (github.com/json-patch/json-patch-tests)'),
    'toml-test': ('MIT', 'COPYING at the pinned commit', 'toml-test, TOML authors (github.com/toml-lang/toml-test)'),
    'cpython-configparser': ('PSF-2.0', 'LICENSE at the pinned commit (Python Software Foundation License)',
                             'CPython Lib/test configparser fixtures, Python Software Foundation'),
    'inih': ('BSD-3-Clause', 'LICENSE.txt at the pinned commit', 'inih, Ben Hoyt (github.com/benhoyt/inih)'),
    'buf-breaking': ('Apache-2.0', 'LICENSE at the pinned commit', 'Buf breaking-change fixtures, Buf Technologies, Inc. '
                     '(github.com/bufbuild/buf, private/bufpkg/bufcheck)'),
    'graphql-js-breaking': ('MIT', 'LICENSE at the pinned commit', 'GraphQL.js, GraphQL Contributors '
                            '(github.com/graphql/graphql-js, src/utilities/__tests__/findBreakingChanges-test.ts)'),
}
SUITE_INFO = {
    'yaml-test-suite': ('YAML Test Suite (data branch)', 'data branch generated 2022-01-17', 'https://github.com/yaml/yaml-test-suite',
                        'standards / conformance suite'),
    'jsontestsuite': ('JSONTestSuite test_parsing', 'master @1ef36fa0 (2024-11-22)', 'https://github.com/nst/JSONTestSuite',
                      'established community / compatibility suite'),
    'json-patch-tests': ('JSON Patch Tests (RFC 6902)', 'master @2a928f90 (2025-02-21)',
                         'https://github.com/json-patch/json-patch-tests', 'established community / compatibility suite'),
    'toml-test': ('toml-test (TOML 1.0.0 manifest)', 'v1.6.0', 'https://github.com/toml-lang/toml-test',
                  'established community / compatibility suite'),
    'cpython-configparser': ('CPython configparser regression fixtures', 'v3.13.18',
                             'https://github.com/python/cpython/blob/v3.13.18/Lib/test/test_configparser.py',
                             'official upstream regression suite'),
    'inih': ('inih regression fixtures', 'master @2bbdec4a (2026-09-27)', 'https://github.com/benhoyt/inih',
             'official upstream regression suite'),
    'buf-breaking': ('Buf breaking-change fixtures', 'v1.75.0',
                     'https://github.com/bufbuild/buf/blob/v1.75.0/private/bufpkg/bufcheck/breaking_test.go',
                     'official upstream regression suite'),
    'graphql-js-breaking': ('GraphQL.js findBreakingChanges tests', 'v16.15.0',
                            'https://github.com/graphql/graphql-js/blob/v16.15.0/src/utilities/__tests__/findBreakingChanges-test.ts',
                            'official upstream regression suite'),
}
DIALECT = {
    'json': 'RFC 8259 JSON (reference: CPython json, strict; NaN/Infinity rejected)',
    'yaml': 'YAML 1.2 (reference: ruamel.yaml 0.18 safe loader, YAML 1.2 Core schema)',
    'toml': 'TOML 1.0.0 (reference: CPython tomllib)',
    'ini-configparser': 'CPython 3.13 configparser (profile named per case)',
    'ini-inih': 'inih default build (INI_ALLOW_MULTILINE=1, inline comments need whitespace before ;)',
    'proto': 'Protocol Buffers source (reference: buf 1.75.0 compiler)',
    'graphql': 'GraphQL SDL (reference: graphql-js 16.15.0)',
}

BLOCKED = [{
    'language_id': 'xml', 'suite_id': 'w3c-xml-conformance', 'name': 'W3C XML Conformance Test Suite',
    'url': 'https://www.w3.org/XML/Test/', 'lanes': ['parser_fidelity', 'change_understanding'],
    'reason': 'BLOCKED: www.w3.org is not reachable from the benchmark sandbox (HTTPS CONNECT refused, 403, '
              '2026-11-17), and the suite has no authoritative GitHub revision; no other XML source was substituted, '
              'so xml has no external seed for either lane.'}]


# --- case construction ---------------------------------------------------------------------

def case(*, case_id, suite, language, dialect, filename, lane, family, upstream_case, record, old, new, derivation,
         claim, assumptions, expected, plan, review, fields):
    witness_rel = f'{WITNESS_DIR}/{case_id}.json'
    record_obj = json.loads((ROOT / witness_rel).read_bytes()) if (ROOT / witness_rel).exists() else None
    if record_obj and (record_obj.get('old_sha256') != old['sha256'] or record_obj.get('new_sha256') != new['sha256']):
        record_obj = None  # stale witness for different bytes: not usable
    verdict = record_obj['verdict'] if record_obj else None
    verbatim = derivation[0]['step'] == 'verbatim_upstream'
    admitted = verdict == 'supports_claim'
    licence, _, attribution = LIC[suite]
    if admitted:
        cls = 'upstream_gold' if verbatim else 'reviewed_external_derived'
        status, reason = 'admitted', f'{review} Witness verdict: {verdict}.'
    else:
        cls, status = 'external_seed_only', 'candidate'
        reason = (f"Witness verdict {verdict}: {record_obj.get('summary')}; not admitted." if record_obj
                  else 'No executed witness for these bytes; not admitted.')
    return {
        'id': case_id, 'suite_id': suite, 'language_id': language, 'dialect': dialect, 'filename': filename,
        'class': cls, 'lane': lane, 'change_family': family,
        'upstream': {'case_id': upstream_case, 'fields': fields, 'record': record},
        'old': old, 'new': new, 'licence': licence, 'attribution': attribution, 'derivation': derivation,
        'behavior': {'claim': claim, 'assumptions': assumptions},
        'oracle': {'basis': 'executable_witness' if record_obj else 'candidate_only',
                   'witness': {'path': witness_rel, 'sha256': sha((ROOT / witness_rel).read_bytes())} if record_obj else None,
                   'byte_identity': old == new,
                   'notes': 'Reference implementation executed outside IntentumDiff by scripts/structured_family.py '
                            'witness; IntentumDiff output is not an input to this oracle.'},
        'admission': {'status': status, 'reason': reason},
        'expected': expected if admitted else None,
        'witness_plan': plan,
    }


VERBATIM = [{'step': 'verbatim_upstream', 'description': 'exact upstream file bytes at the pinned revision on both sides'}]


def parser_case(*, case_id, suite, language, dialect, filename, path, accepts, plan, upstream_case, label):
    ref = upref(suite, path)
    return case(case_id=case_id, suite=suite, language=language, dialect=dialect, filename=filename,
                lane='parser_fidelity', family='accept' if accepts else 'reject', upstream_case=upstream_case,
                record=ref, old=ref, new=ref, derivation=VERBATIM, claim='preserved',
                assumptions=f'Single-source conformance: upstream label `{label}` says the document must be '
                            f'{"accepted" if accepts else "rejected"}; old and new are the identical upstream bytes.',
                expected=expect_accept() if accepts else expect_reject(),
                plan={**plan, 'accepts': accepts}, review=f'Upstream label `{label}` and the executed reference agree.',
                fields=[path])


# JSON ----------------------------------------------------------------------------------------

def _canon(value):
    return json.dumps(value, indent=2, ensure_ascii=False) + '\n'


def _first_leaf(value, path=()):
    if isinstance(value, dict):
        for k, v in value.items():
            found = _first_leaf(v, path + (k,))
            if found:
                return found
    elif isinstance(value, list):
        for i, v in enumerate(value):
            found = _first_leaf(v, path + (i,))
            if found:
                return found
    elif value is None or isinstance(value, (bool, int, float, str)):
        return path, value
    return None


def _set(value, path, leaf):
    if not path:
        return leaf
    import copy
    out = copy.deepcopy(value); cur = out
    for p in path[:-1]:
        cur = cur[p]
    cur[path[-1]] = leaf
    return out


def _bump(leaf):
    if isinstance(leaf, bool):
        return not leaf
    if isinstance(leaf, int):
        return leaf + 1
    if isinstance(leaf, float):
        return leaf * 2 if leaf else 1.0
    if isinstance(leaf, str):
        return leaf + 'x'
    return 0


def _json_load_strict(data):
    def no_constant(name):
        raise ValueError(f'non-RFC constant {name}')
    return json.loads(data.decode('utf-8'), parse_constant=no_constant)


def json_cases():
    out = []; discovery = []
    acq = acquisition('jsontestsuite')
    names = [n for n in acq['enumeration']['listed'] if f'test_parsing/{n}' in acq['files']]
    eligible = 0
    for name in names:
        kind = name[0]
        data = upbytes('jsontestsuite', f'test_parsing/{name}')
        if kind == 'i' or not utf8(data):
            continue  # implementation-defined, or not representable through the UTF-8 text C ABI
        eligible += 1
        out.append(parser_case(case_id=f'jts-{name[:-5]}', suite='jsontestsuite', language='json',
                               dialect=DIALECT['json'], filename='fixture.json', path=f'test_parsing/{name}',
                               accepts=kind == 'y', plan={'oracle': 'json_parse'}, upstream_case=name,
                               label=f'{kind}_ prefix'))
    discovery.append({'suite_id': 'jsontestsuite', 'language_id': 'json', 'lane': 'parser_fidelity',
                      'discovered': len(names), 'eligible': eligible,
                      'eligibility_rule': 'y_ (must accept) and n_ (must reject) files that decode as UTF-8; i_ '
                                          '(implementation-defined) and non-UTF-8 files excluded',
                      'imported_rows': 'all eligible', 'holdout': 'none; enumeration from results/parsing.html is a lower bound'})

    # Change lane, upstream edit specification: JSON Patch doc -> expected.
    patch_rows = []
    for fname in ('tests.json', 'spec_tests.json'):
        for index, row in enumerate(json.loads(upbytes('json-patch-tests', fname))):
            patch_rows.append((fname, index, row))
    eligible = 0
    for fname, index, row in patch_rows:
        if row.get('disabled') or 'expected' not in row or 'error' in row or not isinstance(row.get('doc'), (dict, list)):
            continue
        eligible += 1
        old_b, new_b = _canon(row['doc']).encode(), _canon(row['expected']).encode()
        stem = f"jpt-{fname.split('.')[0].replace('_', '-')}-{index:03}"
        old = write(f'{DERIVED}/json-patch/{stem}-old.json', old_b)
        new = write(f'{DERIVED}/json-patch/{stem}-new.json', new_b)
        changed = row['doc'] != row['expected']
        ops = sorted({op.get('op') for op in row['patch'] if isinstance(op, dict)})
        out.append(case(
            case_id=stem, suite='json-patch-tests', language='json', dialect=DIALECT['json'], filename='fixture.json',
            lane='change_understanding', family='patch_' + '_'.join(o for o in ops if o),
            upstream_case=f"{fname}[{index}]" + (f": {row['comment']}" if row.get('comment') else ''),
            record=upref('json-patch-tests', fname), old=old, new=new,
            derivation=[{'step': 'derived_transformation', 'description': 'upstream `doc` (old) and `expected` (new) '
                         'serialised with json.dumps(indent=2, ensure_ascii=False), member order kept',
                         'tool': 'scripts/structured_family.py'}],
            claim='changed' if changed else 'preserved',
            assumptions='RFC 6902 patch semantics; typed JSON value equality (member order not observable).',
            expected=expect_changed(old_b, new_b, STRUCT_TYPES, f'JSON Patch ops {ops} change the document value')
            if changed else expect_preserved(EDIT_TYPES, 'the patch leaves the document value unchanged'),
            plan={'oracle': 'json_patch', 'file': fname, 'index': index},
            review=f'Upstream JSON Patch case reviewed (ops {ops}).', fields=['doc', 'patch', 'expected']))
    discovery.append({'suite_id': 'json-patch-tests', 'language_id': 'json', 'lane': 'change_understanding',
                      'discovered': len(patch_rows), 'eligible': eligible,
                      'eligibility_rule': 'non-disabled rows with an `expected` document (error rows are patch-failure '
                                          'cases, not before/after pairs) and a container doc',
                      'imported_rows': 'all eligible', 'holdout': 'none'})

    # Derived controls from JSONTestSuite y_ seeds.
    seeds = []
    for name in names:
        if name[0] != 'y':
            continue
        data = upbytes('jsontestsuite', f'test_parsing/{name}')
        try:
            value = _json_load_strict(data)
        except ValueError:
            continue
        pairs = json.loads(data.decode(), object_pairs_hook=lambda p: p)
        seeds.append((name, data, value, pairs))
    reorder = [s for s in seeds if isinstance(s[2], dict) and len(s[2]) >= 2
               and len(s[3]) == len(s[2])][:DERIVED_LIMIT]
    for name, data, value, _ in reorder:
        new_b = _canon(dict(reversed(list(value.items())))).encode()
        stem = f'jts-{name[:-5]}-reorder'
        out.append(_derived_pair('jsontestsuite', 'json', DIALECT['json'], 'fixture.json', stem, name,
                                 f'test_parsing/{name}', data, new_b, 'key_reorder', 'preserved',
                                 'member order reversed and re-indented (json.dumps indent=2)',
                                 'Typed JSON value equality; object member order and whitespace are not observable.',
                                 expect_preserved(EDIT_TYPES, 'reordering members does not add, delete or modify a value'),
                                 {'oracle': 'json_typed'}))
    containers = [s for s in seeds if isinstance(s[2], (dict, list)) and s[2]]
    for name, data, value, _ in containers[:DERIVED_LIMIT]:
        stem = f'jts-{name[:-5]}-reformat'
        out.append(_derived_pair('jsontestsuite', 'json', DIALECT['json'], 'fixture.json', stem, name,
                                 f'test_parsing/{name}', data, _canon(value).encode(), 'whitespace', 'preserved',
                                 're-serialised with json.dumps(indent=2, ensure_ascii=False), member order kept',
                                 'Typed JSON value equality; whitespace, escapes and number spelling are not observable.',
                                 expect_preserved(EDIT_TYPES, 're-serialisation with equal typed value is not an edit'),
                                 {'oracle': 'json_typed'}))
    for name, data, value, _ in containers[:DERIVED_LIMIT]:
        leaf = _first_leaf(value)
        if leaf is None:
            continue
        path, scalar = leaf
        for kind, replacement in (('value', _bump(scalar)), ('type', json.dumps(scalar) if not isinstance(scalar, str) else None)):
            if replacement is None:
                continue
            old_b = _canon(value).encode(); new_b = _canon(_set(value, path, replacement)).encode()
            stem = f'jts-{name[:-5]}-{kind}-change'
            out.append(_derived_pair('jsontestsuite', 'json', DIALECT['json'], 'fixture.json', stem, name,
                                     f'test_parsing/{name}', old_b, new_b, f'scalar_{kind}_change', 'changed',
                                     f'canonical re-serialisation (old); first scalar leaf at pointer {_pointer(path)} '
                                     f'{"bumped" if kind == "value" else "replaced by its JSON text as a string"} (new)',
                                     'Typed JSON value equality; the consumer reads the changed member.',
                                     expect_changed(old_b, new_b, EDIT_TYPES, f'value at {_pointer(path)} changes'),
                                     {'oracle': 'json_typed'}, old_is_upstream=False))
    return out, discovery


def _pointer(path):
    return '/' + '/'.join(str(p).replace('~', '~0').replace('/', '~1') for p in path) if path else ''


def _derived_pair(suite, language, dialect, filename, stem, upstream_case, seed_path, old_b, new_b, family, claim,
                  description, assumptions, expected, plan, old_is_upstream=True, ext=None):
    ext = ext or filename.rsplit('.', 1)[1]
    seed = upref(suite, seed_path)
    old = seed if old_is_upstream and old_b == upbytes(suite, seed_path) else write(f'{DERIVED}/{suite}/{stem}-old.{ext}', old_b)
    new = write(f'{DERIVED}/{suite}/{stem}-new.{ext}', new_b)
    return case(case_id=stem, suite=suite, language=language, dialect=dialect, filename=filename,
                lane='change_understanding', family=family, upstream_case=upstream_case, record=seed, old=old, new=new,
                derivation=[{'step': 'derived_transformation', 'description': description,
                             'tool': 'scripts/structured_family.py'}],
                claim=claim, assumptions=assumptions, expected=expected, plan=plan,
                review=f'Derivation reviewed: {description}.', fields=[seed_path])


# YAML ----------------------------------------------------------------------------------------

_PLAIN = re.compile(rb'^(?P<key>[A-Za-z_][A-Za-z0-9_ -]*): (?P<val>-?\d+|true|false)[ \t]*$', re.M)


def yaml_cases():
    out = []; discovery = []
    acq = acquisition('yaml-test-suite')
    ids = acq['enumeration']['found']
    eligible = 0; pairs = 0; pair_eligible = 0
    for cid in ids:
        files = acq['files']
        if f'{cid}/in.yaml' not in files:
            continue
        data = upbytes('yaml-test-suite', f'{cid}/in.yaml')
        error = f'{cid}/error' in files
        if utf8(data):
            eligible += 1
            stem = 'yts-' + cid.replace('/', '-')
            out.append(parser_case(case_id=stem, suite='yaml-test-suite', language='yaml', dialect=DIALECT['yaml'],
                                   filename='fixture.yaml', path=f'{cid}/in.yaml', accepts=not error,
                                   plan={'oracle': 'yaml_parse', 'in_json': f'{cid}/in.json' if f'{cid}/in.json' in files else None},
                                   upstream_case=cid, label='error marker' if error else 'no error marker'))
        if not error and f'{cid}/out.yaml' in files:
            pairs += 1
            out_b = upbytes('yaml-test-suite', f'{cid}/out.yaml')
            if out_b != data and utf8(data) and utf8(out_b):
                pair_eligible += 1
                stem = 'yts-' + cid.replace('/', '-') + '-out'
                out.append(case(
                    case_id=stem, suite='yaml-test-suite', language='yaml', dialect=DIALECT['yaml'],
                    filename='fixture.yaml', lane='change_understanding', family='canonical_reemission',
                    upstream_case=cid, record=upref('yaml-test-suite', f'{cid}/in.yaml'),
                    old=upref('yaml-test-suite', f'{cid}/in.yaml'), new=upref('yaml-test-suite', f'{cid}/out.yaml'),
                    derivation=[{'step': 'verbatim_upstream', 'description': 'upstream in.yaml (old) and its upstream '
                                 'out.yaml re-emission (new), unmodified'}],
                    claim='preserved', assumptions='YAML 1.2 Core schema; same representation graph (typed value) '
                    'despite presentation (style, indentation, quoting, comments).',
                    expected=expect_preserved(['ADDITION', 'DELETION'],
                                              'a re-emission of the same representation does not add or delete content'),
                    plan={'oracle': 'yaml_typed', 'in_json': f'{cid}/in.json' if f'{cid}/in.json' in files else None},
                    review='Upstream out.yaml is the suite\'s re-emission of in.yaml.', fields=['in.yaml', 'out.yaml']))
    discovery.append({'suite_id': 'yaml-test-suite', 'language_id': 'yaml', 'lane': 'parser_fidelity',
                      'discovered': len(ids), 'eligible': eligible,
                      'eligibility_rule': 'test (sub)directories with in.yaml decodable as UTF-8; `error` marker = must '
                                          'reject. Enumeration INCOMPLETE: no in-suite index; probed candidate IDs only',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    discovery.append({'suite_id': 'yaml-test-suite', 'language_id': 'yaml', 'lane': 'change_understanding',
                      'discovered': pairs, 'eligible': pair_eligible,
                      'eligibility_rule': 'valid tests with out.yaml whose bytes differ from in.yaml',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    # Derived: a plain int/bool scalar re-quoted as a string (type change), and a comment line.
    seeds = []
    for cid in ids:
        if f'{cid}/in.yaml' in acq['files'] and f'{cid}/error' not in acq['files']:
            data = upbytes('yaml-test-suite', f'{cid}/in.yaml')
            if utf8(data) and _PLAIN.search(data) and b'\t' not in data:
                seeds.append((cid, data))
    for cid, data in seeds[:DERIVED_LIMIT]:
        m = _PLAIN.search(data)
        new_b = data[:m.start('val')] + b'"' + m.group('val') + b'"' + data[m.end('val'):]
        stem = 'yts-' + cid.replace('/', '-') + '-quote'
        out.append(_derived_pair('yaml-test-suite', 'yaml', DIALECT['yaml'], 'fixture.yaml', stem, cid, f'{cid}/in.yaml',
                                 data, new_b, 'scalar_type_change', 'changed',
                                 f'plain scalar `{m.group("val").decode()}` of key `{m.group("key").decode()}` double-quoted',
                                 'YAML 1.2 Core schema: a plain int/bool becomes a string; the application does not coerce.',
                                 expect_changed(data, new_b, EDIT_TYPES, 'scalar type changes to string'),
                                 {'oracle': 'yaml_typed'}))
    for cid, data in seeds[:DERIVED_LIMIT]:
        if data.startswith(b'%') or data.startswith(b'---'):
            new_b = data + (b'' if data.endswith(b'\n') else b'\n') + b'# derived trailing comment\n'
        else:
            new_b = b'# derived leading comment\n' + data
        stem = 'yts-' + cid.replace('/', '-') + '-comment'
        out.append(_derived_pair('yaml-test-suite', 'yaml', DIALECT['yaml'], 'fixture.yaml', stem, cid, f'{cid}/in.yaml',
                                 data, new_b, 'comment', 'preserved', 'one full-line comment added',
                                 'YAML comments are presentation details, not content.',
                                 expect_preserved(['DELETION', 'MODIFICATION'], 'adding a comment does not change content'),
                                 {'oracle': 'yaml_typed'}))
    return out, discovery


# TOML ----------------------------------------------------------------------------------------

_TOML_INT = re.compile(rb'^(?P<key>[A-Za-z0-9_-]+)[ \t]*=[ \t]*(?P<val>[1-9][0-9]{3,})[ \t]*$', re.M)
_TOML_ANYINT = re.compile(rb'^(?P<key>[A-Za-z0-9_-]+)[ \t]*=[ \t]*(?P<val>-?[0-9]+)[ \t]*$', re.M)


def toml_cases():
    out = []; discovery = []
    acq = acquisition('toml-test')
    listed = [p[len('tests/'):] for p in sorted(acq['files']) if p.startswith('tests/') and p.endswith('.toml')]
    valid = [p for p in listed if p.startswith('valid/')]; invalid = [p for p in listed if p.startswith('invalid/')]
    eligible = 0
    for rel in valid + invalid:
        data = upbytes('toml-test', f'tests/{rel}')
        if not utf8(data):
            continue
        eligible += 1
        accepts = rel.startswith('valid/')
        stem = 'toml-' + rel[:-5].replace('/', '-')
        out.append(parser_case(case_id=stem, suite='toml-test', language='toml', dialect=DIALECT['toml'],
                               filename='fixture.toml', path=f'tests/{rel}', accepts=accepts,
                               plan={'oracle': 'toml_parse', 'tagged_json': f'tests/{rel[:-5]}.json' if accepts else None},
                               upstream_case=rel, label='valid/' if accepts else 'invalid/'))
    discovery.append({'suite_id': 'toml-test', 'language_id': 'toml', 'lane': 'parser_fidelity',
                      'discovered': len(valid) + len(invalid), 'eligible': eligible,
                      'eligibility_rule': 'every .toml in tests/files-toml-1.0.0 that decodes as UTF-8; valid/ must '
                                          'accept (typed JSON equality), invalid/ must reject',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    seeds = [(rel, upbytes('toml-test', f'tests/{rel}')) for rel in valid]
    seeds = [(r, d) for r, d in seeds if utf8(d) and _TOML_ANYINT.search(d)]
    for rel, data in seeds[:DERIVED_LIMIT]:
        m = _TOML_ANYINT.search(data); base = 'toml-' + rel[:-5].replace('/', '-')
        bumped = str(int(m.group('val')) + 1).encode()
        for kind, repl, claim, fam, desc in (
                ('bump', bumped, 'changed', 'scalar_value_change', 'integer value incremented'),
                ('string', b'"' + m.group('val') + b'"', 'changed', 'scalar_type_change', 'integer re-typed as a string')):
            new_b = data[:m.start('val')] + repl + data[m.end('val'):]
            out.append(_derived_pair('toml-test', 'toml', DIALECT['toml'], 'fixture.toml', f'{base}-{kind}', rel,
                                     f'tests/{rel}', data, new_b, fam, claim,
                                     f'key `{m.group("key").decode()}`: {desc}',
                                     'TOML 1.0 typed decoding; no application coercion.',
                                     expect_changed(data, new_b, EDIT_TYPES, f'typed value of `{m.group("key").decode()}` changes'),
                                     {'oracle': 'toml_typed'}))
        new_b = b'# derived leading comment\n' + data
        out.append(_derived_pair('toml-test', 'toml', DIALECT['toml'], 'fixture.toml', f'{base}-comment', rel,
                                 f'tests/{rel}', data, new_b, 'comment', 'preserved', 'one full-line comment prepended',
                                 'TOML comments are not data.',
                                 expect_preserved(['DELETION', 'MODIFICATION'], 'adding a comment does not change data'),
                                 {'oracle': 'toml_typed'}))
    spelled = [(r, d) for r, d in ((rel, upbytes('toml-test', f'tests/{rel}')) for rel in valid)
               if utf8(d) and _TOML_INT.search(d)]
    for rel, data in spelled[:DERIVED_LIMIT]:
        m = _TOML_INT.search(data); v = m.group('val')
        new_b = data[:m.start('val')] + v[:-3] + b'_' + v[-3:] + data[m.end('val'):]
        out.append(_derived_pair('toml-test', 'toml', DIALECT['toml'], 'fixture.toml',
                                 'toml-' + rel[:-5].replace('/', '-') + '-underscore', rel, f'tests/{rel}', data, new_b,
                                 'literal_respelling', 'preserved', f'integer {v.decode()} respelled with a digit separator',
                                 'TOML 1.0: `8_080` and `8080` are the same integer; literal spelling is not data.',
                                 expect_preserved(['ADDITION', 'DELETION'], 'respelling a literal does not add or delete data'),
                                 {'oracle': 'toml_typed'}))
    return out, discovery


# INI -----------------------------------------------------------------------------------------

CFG_PROFILES = {
    # Option profiles of the upstream test classes that read each fixture.
    'cfgparser.1': {'cls': 'ConfigParser', 'kwargs': {'strict': False, 'inline_comment_prefixes': [';', '#']},
                    'test': 'BasicTestCase.test_read_returns_file_list (CfgParserTestCaseClass defaults)',
                    'check': [['Foo Bar', 'foo', 'newbar']]},
    'cfgparser.2': {'cls': 'RawConfigParser', 'kwargs': {'strict': False, 'comment_prefixes': ['#', ';', '----'],
                                                         'inline_comment_prefixes': ['//'], 'empty_lines_in_values': False},
                    'test': 'RawConfigParserTestSambaConf.test_reading',
                    'check': [['global', 'workgroup', 'MDKGROUP'], ['tmp', 'echo command', 'cat %s; rm %s']]},
    'cfgparser.3': {'cls': 'ConfigParser', 'kwargs': {'strict': False, 'inline_comment_prefixes': [';', '#'],
                                                      'allow_no_value': True},
                    'test': 'ConfigParserTestCaseNoValue.test_cfgparser_dot_3 (allow_no_value=True)',
                    'check': [['more interpolation', 'lets', 'go shopping']]},
}


def ini_cases():
    out = []; discovery = []
    acq = acquisition('cpython-configparser')
    for name in acq['enumeration']['listed']:
        prof = CFG_PROFILES[name]
        out.append(parser_case(case_id=f'cpy-{name.replace(".", "-")}', suite='cpython-configparser', language='ini',
                               dialect=DIALECT['ini-configparser'] + f": {prof['test']}", filename='fixture.ini',
                               path=f'Lib/test/configdata/{name}', accepts=True,
                               plan={'oracle': 'ini_configparser', 'profile': name}, upstream_case=name,
                               label=f"read successfully by {prof['test']}"))
    discovery.append({'suite_id': 'cpython-configparser', 'language_id': 'ini', 'lane': 'parser_fidelity',
                      'discovered': len(acq['enumeration']['listed']), 'eligible': len(acq['enumeration']['listed']),
                      'eligibility_rule': 'configdata fixtures read by test_configparser.py; inline test strings in the '
                                          'module are not extracted in this slice',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    acq = acquisition('inih')
    names = [n for n in acq['enumeration']['listed'] if f'tests/{n}' in acq['files']]
    errors = _inih_baseline_errors()
    eligible = 0
    for name in names:
        e = errors.get(name)
        if e != 0:
            continue  # e>0: syntax error under inih's dialect only; INI has no common reject rule (see notes)
        eligible += 1
        out.append(parser_case(case_id=f'inih-{name[:-4]}', suite='inih', language='ini', dialect=DIALECT['ini-inih'],
                               filename='fixture.ini', path=f'tests/{name}', accepts=True,
                               plan={'oracle': 'ini_inih', 'file': name}, upstream_case=name,
                               label='baseline_multi.txt e=0'))
    discovery.append({'suite_id': 'inih', 'language_id': 'ini', 'lane': 'parser_fidelity',
                      'discovered': len(names), 'eligible': eligible,
                      'eligibility_rule': 'files with e=0 in baseline_multi.txt. Files with e>0 are dialect-specific '
                                          'rejections (e.g. a name without `=` is valid with allow_no_value) and the '
                                          'IntentumDiff ini route declares no dialect, so no reject claim is admitted',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    # Derived change pairs on configparser seeds.
    seed = upbytes('cpython-configparser', 'Lib/test/configdata/cfgparser.2')
    edits = _ini_edits(seed)
    for stem, new_b, fam, claim, desc in edits:
        exp = (expect_changed(seed, new_b, EDIT_TYPES, desc) if claim == 'changed' else
               expect_preserved(['ADDITION', 'DELETION'] if fam != 'comment' else ['DELETION', 'MODIFICATION'],
                                'the effective configuration is unchanged'))
        out.append(_derived_pair('cpython-configparser', 'ini', DIALECT['ini-configparser'] + ': cfgparser.2 profile',
                                 'fixture.ini', f'cpy-cfgparser-2-{stem}', 'cfgparser.2', 'Lib/test/configdata/cfgparser.2',
                                 seed, new_b, fam, claim, desc,
                                 'Effective values (sections, options, raw values) under the upstream cfgparser.2 profile; '
                                 'comments and spacing around delimiters are not observable.',
                                 exp, {'oracle': 'ini_typed', 'profile': 'cfgparser.2'}))
    return out, discovery


def _ini_edits(seed: bytes):
    lines = seed.split(b'\n'); out = []
    opts = [i for i, l in enumerate(lines) if re.match(rb'^\s+[a-z][a-z ]*[a-z] = \S', l)]
    for n, i in enumerate(opts[:6]):
        m = re.match(rb'^(\s+[a-z][a-z ]*[a-z]) = (.*)$', lines[i])
        new = list(lines); new[i] = m.group(1) + b' = ' + m.group(2) + b'_changed'
        out.append((f'value-{n}', b'\n'.join(new), 'value_change', 'changed',
                    f'option `{m.group(1).strip().decode()}` value changed'))
    for n, i in enumerate(opts[6:12]):
        m = re.match(rb'^(\s+[a-z][a-z ]*[a-z]) = (.*)$', lines[i])
        new = list(lines); new[i] = m.group(1) + b'=' + m.group(2)
        out.append((f'spacing-{n}', b'\n'.join(new), 'whitespace', 'preserved',
                    f'spaces around `=` removed for `{m.group(1).strip().decode()}`'))
    section = next(i for i, l in enumerate(lines) if l.startswith(b'['))
    new = list(lines); new.insert(section, b'; derived comment line')
    out.append(('comment', b'\n'.join(new), 'comment', 'preserved', 'one full-line `;` comment inserted before a section'))
    return out


def _inih_baseline_errors():
    text = upbytes('inih', 'tests/baseline_multi.txt').decode()
    return {m.group(1): int(m.group(2)) for m in re.finditer(r'^(\S+\.ini): e=(-?\d+) ', text, re.M)}


# PROTO ---------------------------------------------------------------------------------------

BUF_BASE = 'private/bufpkg/bufcheck/testdata/breaking'
_ANN = re.compile(r'bufanalysistesting\.NewFileAnnotation(NoLocation)?\(\s*t,\s*"([^"]+)",\s*(?:(\d+),\s*(\d+),\s*(\d+),\s*(\d+),\s*)?"([A-Z0-9_]+)"\s*,?\s*\)')


def buf_assertions():
    """{case_dir: [(file, rule, location|None)]} for testBreaking calls made only of annotation helpers."""
    text = upbytes('buf-breaking', 'private/bufpkg/bufcheck/breaking_test.go').decode()
    out = {}
    for m in re.finditer(r'testBreaking\(\s*t,\s*"([^"]+)",(.*?)\n\t\)', text, re.S):
        body = m.group(2)
        anns = [(a.group(2), a.group(7), None if a.group(1) else [int(a.group(i)) for i in range(3, 7)])
                for a in _ANN.finditer(body)]
        if _ANN.sub('', body).strip(' \t\n,') == '':
            out[m.group(1)] = anns
        else:
            out[m.group(1)] = None  # assertion form not parsed (custom helper/argument): not eligible
    return out


def proto_cases():
    out = []; discovery = []
    acq = acquisition('buf-breaking')
    asserts = buf_assertions()
    dirs = acq['enumeration']['listed']
    eligible_pairs = 0; pairs_found = 0; parse_seen = set()
    for d in dirs:
        prev = {p.rsplit('/', 1)[1] for p in acq['files'] if p.startswith(f'{BUF_BASE}/previous/{d}/') and p.endswith('.proto')}
        cur = {p.rsplit('/', 1)[1] for p in acq['files'] if p.startswith(f'{BUF_BASE}/current/{d}/') and p.endswith('.proto')}
        anns = asserts.get(d)
        for f in sorted(prev & cur):
            old_b = upbytes('buf-breaking', f'{BUF_BASE}/previous/{d}/{f}'); new_b = upbytes('buf-breaking', f'{BUF_BASE}/current/{d}/{f}')
            for side, data in (('previous', old_b), ('current', new_b)):
                if data not in parse_seen and utf8(data):
                    parse_seen.add(data)
                    out.append(parser_case(case_id=f'buf-{d}-{side}-{f[:-6]}', suite='buf-breaking', language='proto',
                                           dialect=DIALECT['proto'], filename='fixture.proto',
                                           path=f'{BUF_BASE}/{side}/{d}/{f}', accepts=True,
                                           plan={'oracle': 'proto_build', 'dir': f'{BUF_BASE}/{side}/{d}'},
                                           upstream_case=f'{side}/{d}/{f}', label='compiled by the upstream breaking test'))
            if old_b == new_b:
                continue
            pairs_found += 1
            file_anns = [a for a in (anns or []) if a[0] == f]
            if anns is None or not file_anns:
                continue
            eligible_pairs += 1
            rules = sorted({a[1] for a in file_anns})
            out.append(case(
                case_id=f'buf-{d}-{f[:-6]}', suite='buf-breaking', language='proto', dialect=DIALECT['proto'],
                filename='fixture.proto', lane='change_understanding', family='breaking_' + '_'.join(r.lower() for r in rules),
                upstream_case=f'breaking_test.go testBreaking("{d}") file {f}',
                record=upref('buf-breaking', 'private/bufpkg/bufcheck/breaking_test.go'),
                old=upref('buf-breaking', f'{BUF_BASE}/previous/{d}/{f}'), new=upref('buf-breaking', f'{BUF_BASE}/current/{d}/{f}'),
                derivation=[{'step': 'verbatim_upstream', 'description': 'previous/ (old) and current/ (new) fixture file, unmodified'}],
                claim='changed', assumptions=f'Buf breaking rules {rules} under the case buf.yaml; the upstream test asserts '
                f'{len(file_anns)} annotation(s) on {f}. Compatibility is one semantic dimension.',
                expected=expect_changed(old_b, new_b, STRUCT_TYPES + ['REFACTORING'], f'Buf reports {rules} on {f}'),
                plan={'oracle': 'buf_breaking', 'dir': d, 'file': f, 'assertions': [list(a) for a in anns]},
                review=f'Upstream testBreaking("{d}") assertions parsed: {len(anns)} total.', fields=[f'previous/{d}/{f}', f'current/{d}/{f}']))
    discovery.append({'suite_id': 'buf-breaking', 'language_id': 'proto', 'lane': 'change_understanding',
                      'discovered': pairs_found, 'eligible': eligible_pairs,
                      'eligibility_rule': 'files present (under the probed names) in both previous/ and current/ with '
                                          'different bytes, whose testBreaking call uses only annotation helpers and '
                                          'asserts at least one annotation on that file',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    discovery.append({'suite_id': 'buf-breaking', 'language_id': 'proto', 'lane': 'parser_fidelity',
                      'discovered': len(parse_seen), 'eligible': len(parse_seen),
                      'eligibility_rule': 'distinct fixture files of the paired cases (all must compile)',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    # Derived preservation controls: comment + reformat of previous/<dir>/1.proto.
    seeds = sorted({d for d in dirs if f'{BUF_BASE}/previous/{d}/1.proto' in acq['files']})[:DERIVED_LIMIT]
    for d in seeds:
        data = upbytes('buf-breaking', f'{BUF_BASE}/previous/{d}/1.proto')
        if not utf8(data):
            continue
        new_b = b'// derived comment: documentation only\n\n' + data
        out.append(_derived_pair('buf-breaking', 'proto', DIALECT['proto'], 'fixture.proto', f'buf-{d}-1-comment', d,
                                 f'{BUF_BASE}/previous/{d}/1.proto', data, new_b, 'comment', 'preserved',
                                 'a leading `//` comment and a blank line prepended',
                                 'Descriptor semantics (source info excluded) and every buf breaking rule category.',
                                 expect_preserved(['DELETION', 'MODIFICATION'], 'a comment does not change the descriptor'),
                                 {'oracle': 'proto_equivalent', 'dir': f'{BUF_BASE}/previous/{d}'}))
    return out, discovery


# GRAPHQL -------------------------------------------------------------------------------------

_IT = re.compile(r"\n  it\('((?:[^'\\]|\\.)*)', \(\) => \{\n(.*?)\n  \}\);", re.S)
_SCHEMA = re.compile(r'const (oldSchema|newSchema) = buildSchema\(`(.*?)`\);', re.S)
_EXPECT = re.compile(r'expect\(find(Breaking|Dangerous)Changes\(oldSchema, newSchema\)\)\.to\.deep\.equal\(\[(.*?)\]\);', re.S)
_ENTRY = re.compile(r"type:\s*(Breaking|Dangerous)ChangeType\.(\w+),\s*description:\s*((?:'(?:[^'\\]|\\.)*'\s*\+?\s*)+)", re.S)


def graphql_pairs():
    text = upbytes('graphql-js-breaking', 'src/utilities/__tests__/findBreakingChanges-test.ts').decode()
    out = []
    for index, m in enumerate(_IT.finditer(text)):
        title, body = m.group(1), m.group(2)
        schemas = dict(_SCHEMA.findall(body))
        expects = []
        parsed = True
        for e in _EXPECT.finditer(body):
            inner = e.group(2)
            entries = [(k, t, ''.join(re.findall(r"'((?:[^'\\]|\\.)*)'", d))) for k, t, d in _ENTRY.findall(inner)]
            if len(entries) != inner.count('type:'):
                parsed = False
            expects.append((e.group(1), entries))
        out.append({'index': index, 'title': title, 'old': schemas.get('oldSchema'), 'new': schemas.get('newSchema'),
                    'expects': expects, 'parsed': parsed and bool(expects) and len(schemas) == 2
                    and body.count('buildSchema(') == 2 and '${' not in body})
    return out


def graphql_cases():
    out = []; discovery = []
    pairs = graphql_pairs(); eligible = 0; parse_seen = {}
    for p in pairs:
        if not p['parsed']:
            continue
        expected_changes = [x for _, entries in p['expects'] for x in entries]
        if not expected_changes or p['old'] == p['new']:
            continue
        eligible += 1
        stem = f"gql-{p['index']:02}"
        old_b, new_b = p['old'].encode(), p['new'].encode()
        old = write(f'{DERIVED}/graphql-js-breaking/{stem}-old.graphql', old_b)
        new = write(f'{DERIVED}/graphql-js-breaking/{stem}-new.graphql', new_b)
        types = sorted({f'{k}.{t}' for k, t, _ in expected_changes})
        out.append(case(
            case_id=stem, suite='graphql-js-breaking', language='graphql', dialect=DIALECT['graphql'],
            filename='fixture.graphql', lane='change_understanding', family='schema_change',
            upstream_case=f"findBreakingChanges-test.ts it('{p['title']}')",
            record=upref('graphql-js-breaking', 'src/utilities/__tests__/findBreakingChanges-test.ts'), old=old, new=new,
            derivation=[{'step': 'verbatim_upstream', 'description': 'exact contents of the oldSchema/newSchema buildSchema '
                         'template literals (no interpolation, no escapes) extracted from the TypeScript test wrapper'}],
            claim='changed', assumptions=f'graphql-js 16.15.0 schema comparison; upstream asserts {types}. '
                                         'Compatibility labels, not author intent.',
            expected=expect_changed(old_b, new_b, STRUCT_TYPES + ['REFACTORING'], f'schema change {types}'),
            plan={'oracle': 'graphql_breaking', 'expects': [[k, entries] for k, entries in p['expects']]},
            review=f"Upstream it('{p['title']}') assertions parsed ({len(expected_changes)} change(s)).",
            fields=['oldSchema', 'newSchema', 'expect(...).to.deep.equal']))
        for side, data, ref in (('old', old_b, old), ('new', new_b, new)):
            if data in parse_seen:
                continue
            parse_seen[data] = True
            out.append(case(
                case_id=f'{stem}-{side}-parse', suite='graphql-js-breaking', language='graphql', dialect=DIALECT['graphql'],
                filename='fixture.graphql', lane='parser_fidelity', family='accept', upstream_case=f"it('{p['title']}') {side}Schema",
                record=upref('graphql-js-breaking', 'src/utilities/__tests__/findBreakingChanges-test.ts'), old=ref, new=ref,
                derivation=[{'step': 'verbatim_upstream', 'description': 'exact template-literal SDL on both sides'}],
                claim='preserved', assumptions='The upstream test builds this SDL with buildSchema; it must be accepted.',
                expected=expect_accept(), plan={'oracle': 'graphql_parse', 'accepts': True},
                review='Upstream buildSchema call and executed reference agree.', fields=[f'{side}Schema']))
        comment = b'# derived comment: documentation only\n' + old_b + b'\n'
        out.append(_derived_pair('graphql-js-breaking', 'graphql', DIALECT['graphql'], 'fixture.graphql',
                                 f'{stem}-comment', f"it('{p['title']}')", 'src/utilities/__tests__/findBreakingChanges-test.ts',
                                 old_b, comment, 'comment', 'preserved', 'a leading `#` comment and a trailing newline added',
                                 'Schema equality (lexicographically sorted printSchema); comments are ignored tokens.',
                                 expect_preserved(['DELETION', 'MODIFICATION'], 'a comment does not change the schema'),
                                 {'oracle': 'graphql_equivalent'}, old_is_upstream=False))
    discovery.append({'suite_id': 'graphql-js-breaking', 'language_id': 'graphql', 'lane': 'change_understanding',
                      'discovered': len(pairs), 'eligible': eligible,
                      'eligibility_rule': 'it() blocks with exactly two literal buildSchema calls, parsed expectations '
                                          'and at least one asserted change',
                      'imported_rows': 'all eligible', 'holdout': 'none'})
    discovery.append({'suite_id': 'graphql-js-breaking', 'language_id': 'graphql', 'lane': 'parser_fidelity',
                      'discovered': len(parse_seen), 'eligible': len(parse_seen),
                      'eligibility_rule': 'distinct SDL literals of the eligible pairs', 'imported_rows': 'all eligible',
                      'holdout': 'none'})
    return out, discovery


BUILDERS = {'json': json_cases, 'yaml': yaml_cases, 'toml': toml_cases, 'ini': ini_cases, 'proto': proto_cases,
            'graphql': graphql_cases}
SUITE_LANG = {'jsontestsuite': 'json', 'json-patch-tests': 'json', 'yaml-test-suite': 'yaml', 'toml-test': 'toml',
              'cpython-configparser': 'ini', 'inih': 'ini', 'buf-breaking': 'proto', 'graphql-js-breaking': 'graphql'}


def suite_records():
    out = []
    for suite, (name, version, url, evidence) in SUITE_INFO.items():
        acq = acquisition(suite)
        licence, licence_evidence, attribution = LIC[suite]
        verification = [f"revision resolved from {acq['repository']}@{acq['ref']}: {acq['revision_resolution']}",
                        f"every file fetched from raw.githubusercontent.com at the commit; {len(acq['files'])} files, "
                        'SHA-256 recorded in ACQUISITION.json',
                        f"enumeration: {acq['enumeration']['method']} (complete: {acq['enumeration'].get('complete')})",
                        'BLOCKED: github.com tree listings, the GitHub REST API and git clones are not reachable for this '
                        'repository from the sandbox; no tree-level cross-check was possible'] + acq.get('notes', [])
        out.append({'suite_id': suite, 'name': name, 'version': version, 'url': url, 'revision': acq['revision'],
                    'licence': licence, 'licence_evidence': licence_evidence, 'attribution': attribution,
                    'evidence_class': evidence,
                    'acquisition': {'method': 'scripts/acquire_structured_benchmarks.py (raw.githubusercontent.com at the '
                                              'pinned commit)',
                                    'files': [upref(suite, 'ACQUISITION.json')], 'verification': verification}})
    return out


def define(languages=None):
    cases = []; discovery = []
    for lang, fn in BUILDERS.items():
        if languages and lang not in languages:
            continue
        c, d = fn(); cases += c; discovery += d
    return cases, discovery


def build():
    cases, discovery = define()
    have = {(d['suite_id'], d['language_id'], d['lane']) for d in discovery}
    for key in sorted({(c['suite_id'], c['language_id'], c['lane']) for c in cases} - have):
        discovery.append({'suite_id': key[0], 'language_id': key[1], 'lane': key[2], 'discovered': 0, 'eligible': 0,
                          'eligibility_rule': 'the suite supplies no before/after pairs for this lane; only '
                                              'reviewed_external_derived edits of its seeds (counted on their own rows)',
                          'imported_rows': [], 'holdout': 'none'})
    ids = Counter(c['id'] for c in cases)
    dup = [k for k, n in ids.items() if n > 1]
    if dup:
        raise ValueError(f'duplicate case ids: {dup[:5]}')
    return {'schema_version': 1, 'family': 'core#265 structured/data/schema', 'suites': suite_records(),
            'discovery': discovery, 'cases': cases, 'blocked_acquisitions': BLOCKED}


# --- witness execution -----------------------------------------------------------------------

def _ruamel(script, *args, stdin=None):
    r = subprocess.run([TOOLS['ruamel_python'], '-I', '-c', script, *args], input=stdin, capture_output=True, timeout=60)
    return r.returncode, r.stdout, r.stderr


RUAMEL_LOAD = r'''
import json, sys
from ruamel.yaml import YAML
y = YAML(typ='safe', pure=True)
data = sys.stdin.buffer.read().decode('utf-8')
try:
    docs = list(y.load_all(data))
except Exception as e:
    print(json.dumps({'ok': False, 'error': type(e).__name__ + ': ' + str(e)[:300]})); sys.exit(0)
def norm(v):
    import datetime
    if isinstance(v, dict): return {'map': sorted([[json.dumps(norm(k), sort_keys=True), norm(x)] for k, x in v.items()])}
    if isinstance(v, list): return [norm(x) for x in v]
    if isinstance(v, (datetime.date, datetime.datetime)): return {'ts': v.isoformat()}
    if isinstance(v, bytes): return {'binary': v.hex()}
    if isinstance(v, float) and v != v: return {'float': 'nan'}
    if isinstance(v, float) and v in (float('inf'), float('-inf')): return {'float': str(v)}
    if isinstance(v, set): return {'set': sorted(json.dumps(norm(x), sort_keys=True) for x in v)}
    return [type(v).__name__, v] if not isinstance(v, (dict, list)) else v
print(json.dumps({'ok': True, 'docs': [norm(d) for d in docs], 'plain': [d for d in docs] if all(isinstance(d,(dict,list,str,int,float,bool,type(None))) for d in docs) else None}, default=str))
'''


def _yaml_load(data):
    code, out, err = _ruamel(RUAMEL_LOAD, stdin=data)
    if code:
        return {'ok': None, 'error': err.decode(errors='replace')[-300:]}
    return json.loads(out)


def _json_stream(text):
    dec = json.JSONDecoder(); i = 0; docs = []
    while True:
        while i < len(text) and text[i] in ' \t\r\n':
            i += 1
        if i >= len(text):
            return docs
        value, i = dec.raw_decode(text, i); docs.append(value)


def _typed_equal_json(a, b):
    return json.dumps(a, sort_keys=True) == json.dumps(b, sort_keys=True)


def _toml_tagged(value):
    """Convert tomllib output into toml-test's tagged JSON shape for comparison."""
    import datetime
    if isinstance(value, dict):
        return {k: _toml_tagged(v) for k, v in value.items()}
    if isinstance(value, list):
        return [_toml_tagged(v) for v in value]
    if isinstance(value, bool):
        return {'type': 'bool', 'value': 'true' if value else 'false'}
    if isinstance(value, int):
        return {'type': 'integer', 'value': str(value)}
    if isinstance(value, float):
        return {'type': 'float', 'value': value}
    if isinstance(value, str):
        return {'type': 'string', 'value': value}
    if isinstance(value, datetime.datetime):
        return {'type': 'datetime' if value.tzinfo else 'datetime-local', 'value': value}
    if isinstance(value, datetime.date):
        return {'type': 'date-local', 'value': value}
    if isinstance(value, datetime.time):
        return {'type': 'time-local', 'value': value}
    raise TypeError(type(value))


def _tagged_equal(actual, expected):
    import datetime
    if isinstance(expected, dict) and set(expected) == {'type', 'value'} and isinstance(expected['value'], str):
        if not isinstance(actual, dict) or actual.get('type') != expected['type']:
            return False
        a, e = actual['value'], expected['value']
        if expected['type'] == 'float':
            ev = {'inf': float('inf'), '+inf': float('inf'), '-inf': float('-inf')}.get(e)
            if e in ('nan', '+nan', '-nan'):
                return a != a
            return a == (ev if ev is not None else float(e))
        if expected['type'] in ('datetime', 'datetime-local', 'date-local', 'time-local'):
            parse = {'datetime': datetime.datetime.fromisoformat, 'datetime-local': datetime.datetime.fromisoformat,
                     'date-local': datetime.date.fromisoformat, 'time-local': datetime.time.fromisoformat}[expected['type']]
            try:
                return a == parse(e.replace('z', 'Z').replace(' ', 'T') if 'time' not in expected['type'] or
                                  expected['type'] != 'time-local' else e)
            except ValueError:
                return False
        return a == e
    if isinstance(expected, dict):
        return isinstance(actual, dict) and set(actual) == set(expected) and all(_tagged_equal(actual[k], expected[k]) for k in expected)
    if isinstance(expected, list):
        return isinstance(actual, list) and len(actual) == len(expected) and all(map(_tagged_equal, actual, expected))
    return False


def _cfg_values(data, profile):
    prof = CFG_PROFILES[profile]
    kwargs = {k: tuple(v) if isinstance(v, list) else v for k, v in prof['kwargs'].items()}
    cp = getattr(configparser, prof['cls'])(**kwargs)
    cp.read_string(data.decode('utf-8'))
    raw = {s: {k: cp.get(s, k, raw=True) for k in cp.options(s)} for s in cp.sections()}
    raw[cp.default_section] = dict(cp.defaults())
    return cp, raw


_INIH_OUTPUT = None


def _inih_run():
    """Compile and run inih's own unittest (default build) and compare with the upstream baseline."""
    global _INIH_OUTPUT
    if _INIH_OUTPUT is None:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp) / 'inih'; shutil.copytree(ROOT / UP / 'inih', base)
            r = subprocess.run([TOOLS['cc'], '-Wall', '../ini.c', 'unittest.c', '-o', 'unittest_multi'],
                               cwd=base / 'tests', capture_output=True, text=True)
            if r.returncode:
                _INIH_OUTPUT = {'ok': False, 'error': r.stderr[-500:]}
            else:
                run = subprocess.run(['./unittest_multi'], cwd=base / 'tests', capture_output=True, timeout=60)
                baseline = upbytes('inih', 'tests/baseline_multi.txt')
                _INIH_OUTPUT = {'ok': True, 'matches_baseline': run.stdout == baseline,
                                'errors': {m.group(1).decode(): int(m.group(2)) for m in
                                           re.finditer(rb'^(\S+\.ini): e=(-?\d+) ', run.stdout, re.M)},
                                'cc': subprocess.run([TOOLS['cc'], '--version'], capture_output=True, text=True).stdout.splitlines()[0]}
    return _INIH_OUTPUT


def _node(script, payload):
    env = {**os.environ, 'NODE_PATH': TOOLS['node_modules']}
    r = subprocess.run(['node', '-e', script], input=json.dumps(payload), capture_output=True, text=True, env=env, timeout=60)
    if r.returncode:
        return {'ok': None, 'error': r.stderr[-400:]}
    return json.loads(r.stdout)


GQL = r'''
const g = require('graphql'); let s = ''; process.stdin.on('data', d => s += d); process.stdin.on('end', () => {
  const p = JSON.parse(s); const out = {version: g.version};
  try {
    if (p.mode === 'parse') { g.buildSchema(p.old); out.ok = true; }
    else if (p.mode === 'breaking') {
      const a = g.buildSchema(p.old), b = g.buildSchema(p.new);
      out.breaking = g.findBreakingChanges(a, b); out.dangerous = g.findDangerousChanges(a, b); out.ok = true;
    } else {
      const pr = x => g.printSchema(g.lexicographicSortSchema(g.buildSchema(x)));
      out.equal = pr(p.old) === pr(p.new); out.ok = true;
    }
  } catch (e) { out.ok = false; out.error = String(e).slice(0, 300); }
  process.stdout.write(JSON.stringify(out));
});
'''


def _buf():
    return str(Path(TOOLS['node_modules']) / '.bin' / 'buf')


def _buf_breaking(d):
    with tempfile.TemporaryDirectory() as tmp:
        for side in ('previous', 'current'):
            shutil.copytree(ROOT / UP / 'buf-breaking' / BUF_BASE / side / d, Path(tmp) / side)
        r = subprocess.run([_buf(), 'breaking', '.', '--against', '../previous', '--error-format', 'json'],
                           cwd=Path(tmp) / 'current', capture_output=True, text=True, timeout=120)
        anns = []
        for line in r.stdout.splitlines():
            try:
                a = json.loads(line)
            except ValueError:
                continue
            anns.append((a.get('path'), a.get('type'), [a.get('start_line'), a.get('start_column'), a.get('end_line'),
                                                        a.get('end_column')] if a.get('start_line') else None))
        return r.returncode, anns, r.stderr[-400:]


def _buf_image(directory: Path):
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / 'image.binpb'
        r = subprocess.run([_buf(), 'build', str(directory), '--exclude-source-info', '-o', str(out)],
                           capture_output=True, text=True, timeout=120)
        return (out.read_bytes() if r.returncode == 0 else None), r.stderr[-400:]


def run_witness(c):
    plan = c['witness_plan']; oracle = plan['oracle']
    old = (ROOT / c['old']['path']).read_bytes(); new = (ROOT / c['new']['path']).read_bytes()
    claim = c['behavior']['claim']; obs = {}; tool = None
    try:
        if oracle in ('json_parse', 'toml_parse', 'yaml_parse', 'ini_configparser', 'ini_inih', 'proto_build', 'graphql_parse'):
            accepts = plan['accepts']
            if oracle == 'json_parse':
                tool = f'CPython {sys.version.split()[0]} json (strict, parse_constant rejects NaN/Infinity)'
                try:
                    _json_load_strict(old); got = True
                except (ValueError, RecursionError) as e:
                    got = False; obs['error'] = str(e)[:200]
            elif oracle == 'toml_parse':
                tool = f'CPython {sys.version.split()[0]} tomllib (TOML 1.0.0)'
                try:
                    value = tomllib.loads(old.decode('utf-8')); got = True
                except (tomllib.TOMLDecodeError, UnicodeDecodeError, ValueError) as e:
                    got = False; obs['error'] = str(e)[:200]
                if got and plan.get('tagged_json'):
                    expected = json.loads(upbytes('toml-test', plan['tagged_json']))
                    obs['typed_json_equal'] = _tagged_equal(_toml_tagged(value), expected)
                    if not obs['typed_json_equal']:
                        return _record(c, 'inconclusive', tool, obs, 'reference accepted but typed value differs from upstream JSON')
            elif oracle == 'yaml_parse':
                tool = 'ruamel.yaml 0.18.16 safe loader (pure), YAML 1.2'
                res = _yaml_load(old)
                if res.get('ok') is None:
                    return _record(c, 'inconclusive', tool, res, 'reference loader crashed')
                got = res['ok']; obs['error'] = res.get('error')
                if got and plan.get('in_json'):
                    try:
                        expected = _json_stream(upbytes('yaml-test-suite', plan['in_json']).decode())
                        obs['in_json_equal'] = res.get('plain') is not None and _typed_equal_json(res['plain'], expected)
                    except ValueError as e:
                        obs['in_json_equal'] = None; obs['in_json_error'] = str(e)
                    if obs['in_json_equal'] is False:
                        return _record(c, 'inconclusive', tool, obs, 'reference accepted but value differs from upstream in.json')
            elif oracle == 'ini_configparser':
                tool = f'CPython {sys.version.split()[0]} configparser, profile {plan["profile"]}'
                try:
                    cp, raw = _cfg_values(old, plan['profile']); got = True
                    checks = CFG_PROFILES[plan['profile']]['check']
                    obs['upstream_assertions'] = [[s, k, v, cp.get(s, k) == v] for s, k, v in checks]
                    if not all(x[3] for x in obs['upstream_assertions']):
                        return _record(c, 'inconclusive', tool, obs, 'upstream value assertions not reproduced')
                except configparser.Error as e:
                    got = False; obs['error'] = str(e)[:200]
            elif oracle == 'ini_inih':
                res = _inih_run(); tool = f"inih unittest.c default build ({res.get('cc')})"
                if not res.get('ok') or not res.get('matches_baseline'):
                    return _record(c, 'inconclusive', tool, {k: v for k, v in res.items() if k != 'errors'},
                                   'inih unittest output does not reproduce baseline_multi.txt')
                got = res['errors'].get(plan['file']) == 0; obs['e'] = res['errors'].get(plan['file'])
            elif oracle == 'proto_build':
                tool = 'buf 1.75.0 build'
                image, err = _buf_image(ROOT / UP / 'buf-breaking' / plan['dir'])
                got = image is not None; obs['stderr'] = err if not got else None
            else:
                res = _node(GQL, {'mode': 'parse', 'old': old.decode()}); tool = f"graphql-js {res.get('version')} buildSchema"
                if res.get('ok') is None:
                    return _record(c, 'inconclusive', tool, res, 'reference crashed')
                got = res['ok']; obs['error'] = res.get('error')
            obs['reference_accepts'] = got; obs['upstream_accepts'] = accepts
            verdict = 'supports_claim' if got == accepts else 'contradicts_claim'
            return _record(c, verdict, tool, obs, 'reference and upstream label agree' if got == accepts
                           else 'reference disagrees with the upstream label')
        # change lane
        if oracle == 'json_typed':
            tool = f'CPython {sys.version.split()[0]} json typed equality'
            same = _typed_equal_json(_json_load_strict(old), _json_load_strict(new))
        elif oracle == 'json_patch':
            tool = 'jsonpatch 1.33 apply_patch + CPython json typed equality'
            row = json.loads(upbytes('json-patch-tests', plan['file']))[plan['index']]
            code, out, err = _ruamel('import json,sys,jsonpatch; r=json.load(sys.stdin); '
                                     'print(json.dumps(jsonpatch.apply_patch(r["doc"], r["patch"])))', stdin=json.dumps(row).encode())
            if code:
                return _record(c, 'inconclusive', tool, {'error': err.decode()[-300:]}, 'reference patch application failed')
            applied = json.loads(out)
            obs['applied_equals_upstream_expected'] = _typed_equal_json(applied, row['expected'])
            if not obs['applied_equals_upstream_expected']:
                return _record(c, 'inconclusive', tool, obs, 'reference does not reproduce upstream expected')
            same = _typed_equal_json(_json_load_strict(old), _json_load_strict(new))
            obs['case_bytes_match_upstream'] = (_typed_equal_json(_json_load_strict(old), row['doc'])
                                                and _typed_equal_json(_json_load_strict(new), row['expected']))
        elif oracle == 'yaml_typed':
            tool = 'ruamel.yaml 0.18.16 safe loader (pure), YAML 1.2 typed equality'
            a, b = _yaml_load(old), _yaml_load(new)
            if not (a.get('ok') and b.get('ok')):
                return _record(c, 'inconclusive', tool, {'old': a.get('error'), 'new': b.get('error')}, 'reference could not load both sides')
            same = a['docs'] == b['docs']
            if plan.get('in_json') and a.get('plain') is not None:
                obs['old_equals_in_json'] = _typed_equal_json(a['plain'], _json_stream(upbytes('yaml-test-suite', plan['in_json']).decode()))
        elif oracle == 'toml_typed':
            tool = f'CPython {sys.version.split()[0]} tomllib typed equality'
            same = _toml_tagged(tomllib.loads(old.decode())) == _toml_tagged(tomllib.loads(new.decode()))
        elif oracle == 'ini_typed':
            tool = f'CPython {sys.version.split()[0]} configparser effective raw values, profile {plan["profile"]}'
            same = _cfg_values(old, plan['profile'])[1] == _cfg_values(new, plan['profile'])[1]
        elif oracle == 'buf_breaking':
            tool = 'buf 1.75.0 breaking (case buf.yaml)'
            code, anns, err = _buf_breaking(plan['dir'])
            want = [list(a) for a in plan['assertions']]
            obs.update(exit=code, annotations=[list(a) for a in anns], stderr=err or None, upstream_assertions=want)
            # Located assertions must match exactly; an upstream NoLocation assertion matches the
            # same file+rule at any location. Every reported annotation must be consumed.
            remaining = [list(a) for a in anns]; matched = True
            for w in sorted(want, key=lambda a: a[2] is None):
                hit = next((g for g in remaining if g[0] == w[0] and g[1] == w[1] and (w[2] is None or g[2] == w[2])), None)
                if hit is None:
                    matched = False; break
                remaining.remove(hit)
            if not matched or remaining:
                return _record(c, 'inconclusive', tool, obs, 'buf does not reproduce the upstream assertion set')
            same = not any(a[0] == plan['file'] for a in anns)
        elif oracle == 'proto_equivalent':
            tool = 'buf 1.75.0 build --exclude-source-info image equality + breaking (FILE, WIRE_JSON)'
            with tempfile.TemporaryDirectory() as tmp:
                for side, data in (('previous', old), ('current', new)):
                    shutil.copytree(ROOT / UP / 'buf-breaking' / plan['dir'], Path(tmp) / side)
                    (Path(tmp) / side / '1.proto').write_bytes(data)
                    (Path(tmp) / side / 'buf.yaml').write_text('version: v1\nbreaking:\n  use:\n    - FILE\n    - WIRE_JSON\n')
                ia, ea = _buf_image(Path(tmp) / 'previous'); ib, eb = _buf_image(Path(tmp) / 'current')
                if ia is None or ib is None:
                    return _record(c, 'inconclusive', tool, {'old': ea, 'new': eb}, 'reference could not build both sides')
                r = subprocess.run([_buf(), 'breaking', 'current', '--against', 'previous'], cwd=tmp,
                                   capture_output=True, text=True, timeout=120)
                obs.update(image_equal=ia == ib, breaking_exit=r.returncode, breaking_output=r.stdout[-400:] or None)
                same = ia == ib and r.returncode == 0
        elif oracle == 'graphql_breaking':
            res = _node(GQL, {'mode': 'breaking', 'old': old.decode(), 'new': new.decode()})
            tool = f"graphql-js {res.get('version')} findBreakingChanges/findDangerousChanges"
            if not res.get('ok'):
                return _record(c, 'inconclusive', tool, res, 'reference failed')
            got = {'Breaking': sorted((x['type'], x['description']) for x in res['breaking']),
                   'Dangerous': sorted((x['type'], x['description']) for x in res['dangerous'])}
            obs['reference'] = got; obs['upstream'] = plan['expects']
            for kind, entries in plan['expects']:
                if sorted((t, d.replace("\\'", "'")) for _, t, d in entries) != got[kind]:
                    return _record(c, 'inconclusive', tool, obs, f'{kind} changes differ from the upstream assertion')
            same = not (got['Breaking'] or got['Dangerous'])
        elif oracle == 'graphql_equivalent':
            res = _node(GQL, {'mode': 'equal', 'old': old.decode(), 'new': new.decode()})
            tool = f"graphql-js {res.get('version')} lexicographicSortSchema+printSchema equality"
            if not res.get('ok'):
                return _record(c, 'inconclusive', tool, res, 'reference failed')
            same = res['equal']
        else:
            raise ValueError(f'unknown oracle {oracle}')
    except (ValueError, UnicodeDecodeError, tomllib.TOMLDecodeError, configparser.Error, subprocess.TimeoutExpired) as e:
        return _record(c, 'inconclusive', tool, {'error': f'{type(e).__name__}: {e}'[:300]}, 'reference execution failed')
    obs['reference_equal'] = same
    verdict = 'supports_claim' if same == (claim == 'preserved') else 'contradicts_claim'
    return _record(c, verdict, tool, obs, f"reference says {'equal' if same else 'different'}; claim {claim}")


def _record(c, verdict, tool, obs, summary):
    return {'case_id': c['id'], 'old_sha256': c['old']['sha256'], 'new_sha256': c['new']['sha256'],
            'verdict': verdict, 'oracle': c['witness_plan']['oracle'], 'reference': tool, 'observations': obs,
            'summary': summary, 'executed_utc': datetime.now(timezone.utc).isoformat(timespec='seconds')}


def witness(languages):
    cases, _ = define(languages)
    tally = Counter()
    from concurrent.futures import ThreadPoolExecutor
    with ThreadPoolExecutor(8) as pool:
        records = list(pool.map(run_witness, cases))
    for c, rec in zip(cases, records):
        target = ROOT / WITNESS_DIR / f"{c['id']}.json"
        previous = json.loads(target.read_bytes()) if target.exists() else None
        if previous and {k: v for k, v in previous.items() if k != 'executed_utc'} == \
                {k: v for k, v in rec.items() if k != 'executed_utc'}:
            rec = previous  # unchanged result: keep the record byte-stable
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(encode(rec))
        tally[(c['language_id'], c['lane'], rec['verdict'])] += 1
    for key in sorted(tally):
        print(*key, tally[key])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    w = sub.add_parser('witness'); w.add_argument('--language', nargs='+', choices=sorted(BUILDERS))
    b = sub.add_parser('build'); b.add_argument('--check', action='store_true')
    args = parser.parse_args(argv)
    global CHECK_ONLY
    if args.command == 'witness':
        witness(args.language)
        return 0
    CHECK_ONLY = args.check
    try:
        data = encode(build())
    except ValueError as error:
        print(f'structured family build failed: {error}', file=sys.stderr)
        return 1
    if args.check:
        if not FRAGMENT.exists() or FRAGMENT.read_bytes() != data:
            print('structured family fragment is stale; re-run scripts/structured_family.py build', file=sys.stderr)
            return 1
        return 0
    FRAGMENT.parent.mkdir(parents=True, exist_ok=True)
    FRAGMENT.write_bytes(data)
    frag = json.loads(data)
    tally = Counter((c['language_id'], c['lane'], c['class'], c['admission']['status']) for c in frag['cases'])
    for key in sorted(tally):
        print(*key, tally[key])
    return 0


if __name__ == '__main__':
    sys.exit(main())
