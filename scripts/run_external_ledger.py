#!/usr/bin/env python3
"""Execute admitted schema-2 ledger cases through the production Rust C ABI (#268).

Benchmark tooling only. Execution reuses ``scripts/assess_languages.py``: the same
per-language isolated worker, the same ``live_diff_contents`` + ``parse_to_tree`` C ABI
calls, the same staged-component identity checks, tree/range validity, change-group
reference, completeness and determinism checks. On top of that, each review is judged
against the ledger's independent required/forbidden/alternative results
(``external_ledger.judge``). Nothing here rewrites an expectation from engine output.

Exit status: 2 invalid input/initialisation; 1 a required gate failed; 0 otherwise.
The gate always fails on a blocked case. With ``--baseline`` it fails when an
``upstream_gold`` case that passed in the reviewed baseline no longer passes; without
one, on any ``upstream_gold`` case that fails and has no linked known issue. ``--strict``
fails on any non-passing admitted case. Known failures are reported, never hidden.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent


def _load(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    return module


assess = _load('assess_languages', 'assess_languages.py')
ledger_lib = _load('external_ledger', 'external_ledger.py')


def to_probe(case, root):
    """Translate an admitted ledger case into the assess_languages probe shape."""
    old = ledger_lib.checked_file(root, case['old'], case['id']).decode('utf-8')
    new = ledger_lib.checked_file(root, case['new'], case['id']).decode('utf-8')
    c = case['expected']['classification']
    return {'id': case['id'], 'language_id': case['language_id'], 'filename': case['filename'],
            'category': case['change_family'], 'old': old, 'new': new,
            'expected': {k: c[k] for k in ('has_semantic_changes', 'is_style_only', 'changes_empty', 'completeness')},
            'provenance': {'class': case['class']}, 'related_issues': [case['known_issue']] if case.get('known_issue') else []}


def classify_parse(case, row):
    """Parser-fidelity adapter (#265): judge acceptance/rejection of one upstream document.

    A must-accept document has to be reviewed without fallback, parse diagnostics, tree or
    determinism errors, and with no invented change (the identity review is also judged by
    ``external_ledger.judge``). A must-reject document passes only when the C ABI reports
    it: an explicit fallback/error, or parse diagnostics. Silent acceptance fails.
    """
    status = row.get('status')
    if case['expected']['parse']['accepts']:
        if row.get('explicit_fallback') is not None:
            return 'failed', ['valid document rejected: explicit fallback ' + json.dumps(row['explicit_fallback'])[:300]]
        return None
    if status in ('bounded_explicit_fallback', 'bounded_explicit_error'):
        return 'passed', []
    if status == 'bounded_incomplete_observation':
        if row.get('handling') == 'diagnostics_present':
            return 'passed', []
        return 'failed', ['invalid document accepted without diagnostics (no parse_errors, no fallback)']
    errors = list(row.get('tree_errors') or []) + list(row.get('review_errors') or []) \
        + list(row.get('classification_errors') or [])
    if row.get('parse_errors'):
        errors.insert(0, 'diagnostics present, but the review/tree is invalid')
    return 'failed', errors or [f'unexpected worker status {status}: {row.get("error")}']


def classify(case, row, root):
    """Map one assess_languages observation plus the ledger oracle to a run status."""
    status = row.get('status')
    if status == 'not_executed':
        return 'blocked', [row.get('error') or 'worker did not report this case']
    if case['lane'] == 'parser_fidelity':
        verdict = classify_parse(case, row)
        if verdict is not None:
            return verdict
    if row.get('explicit_fallback') is not None:
        return 'unsupported', ['explicit fallback: ' + json.dumps(row['explicit_fallback'])[:300]]
    if status == 'runtime_error':
        return 'failed', [row.get('error') or 'runtime error']
    errors = list(row.get('classification_errors') or []) + list(row.get('tree_errors') or []) \
        + list(row.get('review_errors') or [])
    errors += [f'parse error: {e}' for e in row.get('parse_errors') or []]
    if row.get('deterministic') is False:
        errors.append('non-deterministic review across repeated calls')
    review = row.get('review')
    if not isinstance(review, dict):
        return 'failed', errors or ['no review returned']
    old = (root / case['old']['path']).read_bytes(); new = (root / case['new']['path']).read_bytes()
    errors += ledger_lib.judge(case, review, old, new)
    # Deduplicate while preserving order: classification is checked by both layers.
    seen = set(); errors = [e for e in errors if not (e in seen or seen.add(e))]
    return ('passed' if not errors else 'failed'), errors


def summarise_review(review):
    changes = review.get('changes') if isinstance(review, dict) else None
    if not isinstance(changes, list):
        return None
    def node(n):
        return None if not isinstance(n, dict) else {'type': n.get('node_type') or n.get('type'),
                                                    'label': n.get('label'), 'position': n.get('position')}
    return {'has_semantic_changes': review.get('has_semantic_changes'), 'is_style_only': review.get('is_style_only'),
            'completeness': review.get('completeness'), 'language': review.get('language'),
            'changes': [{'change_type': c.get('change_type'), 'old_node': node(c.get('old_node')),
                         'new_node': node(c.get('new_node'))} for c in changes if isinstance(c, dict)]}


def gate_failures(results, baseline=None, *, strict=False, narrowed=False):
    """Return (case_id, status, errors) tuples that fail the run."""
    # Mandatory environment failures always fail: a blocked case cannot silently pass.
    gate = [(r['case_id'], r['status'], r['errors']) for r in results if r['status'] == 'blocked']
    if strict:
        return gate + [(r['case_id'], r['status'], r['errors']) for r in results
                       if r['status'] not in ('passed', 'blocked')]
    if baseline is not None:
        # Regression gate: an upstream_gold case that passed in the reviewed baseline must still pass.
        current = {r['case_id']: r for r in results}
        for old in baseline['cases']:
            if old['class'] != 'upstream_gold' or old['status'] != 'passed':
                continue
            now = current.get(old['case_id'])
            if now is None and narrowed:
                continue  # deliberately narrowed run
            if now is None or now['status'] != 'passed':
                gate.append((old['case_id'], 'regressed' if now else 'missing',
                             now['errors'] if now else ['not executed']))
        return gate
    return gate + [(r['case_id'], r['status'], r['errors']) for r in results
                   if r['class'] == 'upstream_gold' and r['status'] != 'passed' and not r.get('known_issue')]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--fixtures', type=Path, default=Path('tests/fixtures/external'))
    parser.add_argument('--parsers', type=Path, required=True, help='staged components with parser_manifest.json')
    parser.add_argument('--native-library', type=Path, help='Rust C ABI library (default: installed transport)')
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--languages', nargs='+')
    parser.add_argument('--classes', nargs='+', choices=ledger_lib.CLASSES)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--timeout', type=int, default=300)
    parser.add_argument('--strict', action='store_true', help='also fail on any failed admitted case of any class')
    parser.add_argument('--baseline', type=Path,
                        help='reviewed runner report; fail only when a previously passing upstream_gold case regresses')
    args = parser.parse_args(argv)
    root = args.fixtures.resolve()
    try:
        ledger, inventory = ledger_lib.validate(root)
        library = assess.native_library_path(args.native_library)
        staged_raw = (args.parsers / 'parser_manifest.json').read_bytes(); staged = json.loads(staged_raw)
        if staged['parsers'] != inventory['parsers'] or staged['extension_index'] != inventory['extension_index']:
            raise ValueError('staged routing differs from fixed inventory')
        components = {a['component']: a['sha256'] for a in staged['components']}
        for entry in inventory['parsers'].values():
            if assess.sha((args.parsers / entry['wasm']).read_bytes()) != components[entry['wasm']]:
                raise ValueError(f"component identity mismatch: {entry['wasm']}")
    except (ledger_lib.LedgerError, OSError, ValueError, KeyError) as error:
        print(f'external ledger run: invalid input: {error}', file=sys.stderr)
        return 2
    selected = [c for c in ledger['cases'] if c['admission']['status'] == 'admitted'
                and (not args.languages or c['language_id'] in args.languages)
                and (not args.classes or c['class'] in args.classes)]
    probes = {c['id']: to_probe(c, root) for c in selected}
    worker_args = argparse.Namespace(parsers=args.parsers, native_library=args.native_library, timeout=args.timeout)
    languages = sorted({c['language_id'] for c in selected})
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        groups = list(pool.map(lambda lang: assess.run_group(
            lang, [probes[c['id']] for c in selected if c['language_id'] == lang], worker_args), languages))
    rows = {}; workers = {}
    for group in groups:
        workers[group['language_id']] = {k: group.get(k) for k in ('worker_error', 'native_library_sha256',
                                                                   'completed_count', 'wall_seconds', 'peak_rss_kib')}
        for row in group['cases']:
            rows[row['id']] = row
    native_sha = assess.sha(library.read_bytes())
    results = []
    for case in selected:
        row = rows.get(case['id'], {'status': 'not_executed', 'error': 'missing observation'})
        if workers.get(case['language_id'], {}).get('native_library_sha256') not in (None, native_sha):
            status, errors = 'blocked', ['worker native library differs from the parent']
        else:
            status, errors = classify(case, row, root)
        results.append({'case_id': case['id'], 'language_id': case['language_id'], 'class': case['class'],
                        'lane': case['lane'], 'suite_id': case['suite_id'], 'change_family': case['change_family'],
                        'behavior_claim': case['behavior']['claim'], 'status': status, 'errors': errors,
                        'known_issue': case.get('known_issue'), 'assess_status': row.get('status'),
                        'completeness': (row.get('review') or {}).get('completeness'),
                        'review_summary': summarise_review(row.get('review')),
                        'elapsed_ms': row.get('elapsed_ms')})
    try:
        commit = subprocess.check_output(['git', '-C', str(HERE.parent), 'rev-parse', 'HEAD'], text=True).strip()
        dirty = bool(subprocess.check_output(['git', '-C', str(HERE.parent), 'status', '--porcelain', '--',
                                              'crates'], text=True).strip())
    except (OSError, subprocess.CalledProcessError):
        commit, dirty = None, None
    report = {'schema_version': 1, 'run_utc': datetime.now(timezone.utc).isoformat(timespec='seconds'),
              'interface': 'production Rust C ABI: live_diff_contents + parse_to_tree (via scripts/assess_languages.py workers)',
              'core_commit': commit, 'core_crates_dirty': dirty,
              'native_library_sha256': native_sha, 'parser_manifest_sha256': assess.sha(staged_raw),
              'ledger_sha256': assess.sha((root / 'admission' / 'ledger.json').read_bytes()),
              'components': components, 'workers': workers,
              'selection': {'languages': args.languages, 'classes': args.classes},
              'cases': results}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_bytes(ledger_lib.encode(report))
    tally = {}
    for r in results:
        tally.setdefault((r['class'], r['status']), 0); tally[(r['class'], r['status'])] += 1
    print(f'{len(results)} admitted cases executed -> {args.report}')
    for key in sorted(tally):
        print(f'  {key[0]:28} {key[1]:12} {tally[key]}')
    baseline = json.loads(args.baseline.read_bytes()) if args.baseline is not None else None
    gate = gate_failures(results, baseline, strict=args.strict, narrowed=bool(args.languages or args.classes))
    for case_id, status, errors in gate:
        print(f"  GATE {case_id} {status}: {'; '.join(errors[:2])}")
    return 1 if gate else 0


if __name__ == '__main__':
    sys.exit(main())
