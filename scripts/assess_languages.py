"""Execute source-judged diagnostic probes for every registered language.

This is assessment tooling, not a semantic engine. Expectations are independently
reviewed fixture data. Actual parsing/review is delegated to the production Rust
C ABI. A bounded set of passing probes is never reported as language certification.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import subprocess
import sys
import time

MEASUREMENTS = frozenset({'duration_ms', 'elapsed_ms', 'wasm_component_cache_hit',
                         'fuel_consumed', 'total_fuel_consumed', 'max_fuel_used_percent',
                         'fuel_used_percent', 'fuel_per_line', 'fuel_per_kb'})


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def stable_review(value, metadata=False):
    if isinstance(value, dict):
        return {key: stable_review(item, metadata or key == 'metadata')
                for key, item in value.items() if not (metadata and key in MEASUREMENTS)}
    if isinstance(value, list):
        return [stable_review(item, metadata) for item in value]
    return value


def check_classification(case, review):
    errors = []
    if review.get('language') != case['language_id']:
        errors.append(f"routing: wanted {case['language_id']}, got {review.get('language')}")
    if review.get('is_fallback') is not False:
        errors.append('fallback or missing explicit fallback flag')
    for key in ('has_semantic_changes', 'is_style_only'):
        actual = review.get(key); wanted = case['expected'].get(key)
        if type(actual) is not bool:
            errors.append(f'{key}: missing explicit boolean')
        elif wanted is not None and actual != wanted:
            errors.append(f'{key}: expected {wanted}, got {actual}')
    changes = review.get('changes')
    if not isinstance(changes, list):
        errors.append('missing changes array')
    else:
        empty = case['expected'].get('changes_empty')
        if case['old'] == case['new'] and changes:
            errors.append('exact identity produced invented changes')
        if empty is not None and bool(not changes) != empty:
            errors.append(f'changes_empty: expected {empty}, got {not changes}')
    return errors


def check_tree(tree, source, *, containment=False):
    """Check structural position validity; this is not exact-span certification."""
    lines = source.encode('utf-8').split(b'\n'); errors = []; ids = set()
    stack = [(tree, None)]
    while stack:
        node, parent = stack.pop()
        if not isinstance(node, dict):
            errors.append('non-object tree node'); continue
        identity = node.get('id')
        if not isinstance(identity, str) or not identity:
            errors.append('missing node id')
        elif identity in ids:
            errors.append(f'duplicate node id {identity}')
        else:
            ids.add(identity)
        p = node.get('position')
        bounds = None
        if not isinstance(p, dict) or any(type(p.get(k)) is not int or p[k] < 0
                for k in ('start_line', 'start_col', 'end_line', 'end_col')):
            errors.append(f'{identity}: missing/nonnegative byte range')
        else:
            start, end = (p['start_line'], p['start_col']), (p['end_line'], p['end_col'])
            if start > end:
                errors.append(f'{identity}: reversed range')
            for line, col in (start, end):
                if line >= len(lines) or col > len(lines[line]):
                    errors.append(f'{identity}: range outside source ({line},{col})')
                else:
                    try:
                        lines[line][:col].decode('utf-8')
                    except UnicodeDecodeError:
                        errors.append(f'{identity}: endpoint splits UTF-8 character')
            bounds = (start, end)
            if containment and parent and (start < parent[0] or end > parent[1]):
                errors.append(f'{identity}: child outside parent range')
        children = node.get('children', [])
        if not isinstance(children, list):
            errors.append(f'{identity}: children is not an array'); continue
        stack.extend((child, bounds) for child in children)
    return errors


def check_review_nodes(review, old, new):
    errors = []
    changes = review.get('changes')
    if not isinstance(changes, list):
        return ['review changes is not an array']
    for index, change in enumerate(changes):
        if not isinstance(change, dict):
            errors.append(f'change {index}: not an object'); continue
        kind = change.get('change_type')
        if not isinstance(kind, str) or not kind:
            errors.append(f'change {index}: missing change type')
        if change.get('old_node') is None and change.get('new_node') is None:
            errors.append(f'change {index}: both source references missing')
        for side, source in (('old', old), ('new', new)):
            if change.get(side + '_node') is not None:
                errors.extend(f'change {index} {side}: {error}' for error in check_tree(change[side + '_node'], source))
    for index, group in enumerate(review.get('change_groups', [])):
        for ref in group.get('raw_change_indices', []):
            if type(ref) is not int or not 0 <= ref < len(changes):
                errors.append(f'group {index}: invalid change reference {ref}')
    return errors


def case_status(case, row):
    incomplete = all(case['expected'].get(k) is None for k in
                     ('has_semantic_changes', 'is_style_only', 'changes_empty'))
    invalid = row['classification_errors'] or row['tree_errors'] or row.get('review_errors') or not row['deterministic']
    if incomplete:
        return 'invalid_result' if invalid else 'bounded_incomplete_observation'
    return 'failed' if invalid or row['parse_errors'] else 'passed'


def check_provenance(case, root):
    p = case['provenance']
    if p.get('class') not in {'handwritten_regression', 'repo_fixture',
                             'reviewed_external_derived', 'upstream_gold'}:
        raise ValueError(f"{case['id']}: unknown provenance class")
    if p['class'] in {'reviewed_external_derived', 'upstream_gold'}:
        if not re.fullmatch('[0-9a-f]{40}', p.get('revision', '')):
            raise ValueError(f"{case['id']}: external source needs immutable revision")
        if not p.get('url') or not p.get('license'):
            raise ValueError(f"{case['id']}: missing external URL/license")
        relative = Path(p.get('path', ''))
        prefix = ('tests', 'fixtures', 'language_assessment')
        if relative.parts[:3] == prefix:
            relative = Path(*relative.parts[3:])
        path = (root / relative).resolve()
        if not path.is_relative_to(root.resolve()) or not path.is_file():
            raise ValueError(f"{case['id']}: external bytes missing/escape root")
        if sha(path.read_bytes()) != p.get('sha256'):
            raise ValueError(f"{case['id']}: external source SHA256 differs")


def load_cases(root, inventory, selected=None):
    cases = []; ids = set(); represented = set(); files = []
    for path in sorted(root.glob('*.json')):
        raw = path.read_bytes(); doc = json.loads(raw)
        if doc.get('schema_version') != 1 or not isinstance(doc.get('cases'), list):
            raise ValueError(f'invalid family document: {path}')
        files.append({'path': path.name, 'sha256': sha(raw)})
        declared = set(doc['languages']); found = {c['language_id'] for c in doc['cases']}
        if declared != found or declared & represented:
            raise ValueError(f'family inventory mismatch/duplicate: {path}')
        represented |= declared
        for case in doc['cases']:
            if case['id'] in ids or case['language_id'] not in inventory['parsers']:
                raise ValueError(f"duplicate ID or unknown language: {case['id']}")
            ids.add(case['id'])
            for key in ('old', 'new', 'filename', 'category'):
                if not isinstance(case.get(key), str):
                    raise ValueError(f"{case['id']}: missing {key}")
            for key in ('has_semantic_changes', 'is_style_only', 'changes_empty'):
                if case['expected'].get(key) is not None and type(case['expected'][key]) is not bool:
                    raise ValueError(f"{case['id']}: invalid oracle {key}")
            if not case.get('oracle', {}).get('basis'):
                raise ValueError(f"{case['id']}: missing independent oracle rationale")
            check_provenance(case, root)
            cases.append(case)
    wanted = set(inventory['parsers'])
    if selected:
        cases = [c for c in cases if c['language_id'] in selected]
        if {c['language_id'] for c in cases} != set(selected):
            raise ValueError('selected language has no probes')
    elif represented != wanted:
        raise ValueError(f'incomplete inventory: missing={sorted(wanted-represented)}, extra={sorted(represented-wanted)}')
    return cases, files


def native_library_path(explicit=None):
    from intentumdiff.rust_core import _extension_path
    return Path(explicit if explicit is not None else _extension_path()).resolve(strict=True)


def worker_transport(explicit=None):
    """Select only the transport's library; all processing remains in Rust."""
    import intentumdiff.rust_core as transport
    path = native_library_path(explicit)
    if explicit is not None:
        transport._extension_path = lambda: str(path)
        transport._C_ABI_LIB = None
    return transport, path


def baseline_cases(baseline, required_languages, selected):
    """Validate the complete reviewed input before narrowing comparison scope."""
    if (not isinstance(baseline, dict) or type(baseline.get('schema_version')) is not int
            or baseline['schema_version'] != 1 or not isinstance(baseline.get('languages'), list)
            or not baseline['languages']):
        raise ValueError('invalid or empty assessment baseline')
    languages = set(); identities = set(); previous = {}
    statuses = {'passed', 'failed', 'invalid_result', 'runtime_error', 'not_executed',
                'bounded_incomplete_observation', 'bounded_explicit_fallback', 'bounded_explicit_error'}
    oracle_keys = ('has_semantic_changes', 'is_style_only', 'changes_empty')
    for language in baseline['languages']:
        if (not isinstance(language, dict) or not isinstance(language.get('language_id'), str)
                or not language['language_id'] or language['language_id'] in languages
                or not isinstance(language.get('case_results'), list) or not language['case_results']):
            raise ValueError('baseline requires unique languages with nonempty case_results')
        language_id = language['language_id']; languages.add(language_id)
        for row in language['case_results']:
            if (not isinstance(row, dict) or not isinstance(row.get('id'), str) or not row['id']
                    or row['id'] in identities or row.get('language_id') != language_id):
                raise ValueError('baseline contains duplicate or misrouted case ID')
            identities.add(row['id'])
            if (not isinstance(row.get('status'), str) or row['status'] not in statuses
                    or any(not isinstance(row.get(key), str) or not re.fullmatch('[0-9a-f]{64}', row[key])
                           for key in ('old_sha256', 'new_sha256'))
                    or not isinstance(row.get('expected'), dict)
                    or any(key not in row['expected'] or
                           (row['expected'][key] is not None and type(row['expected'][key]) is not bool)
                           for key in oracle_keys)):
                raise ValueError('baseline case lacks source/oracle/status identity')
            if selected is None or language_id in selected:
                previous[row['id']] = row
    missing = (required_languages | set(selected or [])) - languages
    if missing or not previous:
        raise ValueError(f'baseline coverage missing for requested scope: {sorted(missing)}')
    return previous


def compare_baseline(baseline, cases, groups, selected, native_sha):
    """Protect reviewed passes and input identity without blessing known failures."""
    previous = baseline_cases(baseline, {c['language_id'] for c in cases}, selected)
    regressions = []; improvements = []

    def reject(reason, language, case_id=None):
        regressions.append({'language_id': language, 'case_id': case_id, 'reason': reason})

    current = {c['id']: c for c in cases}; observed = {}; workers = {}
    for group in groups:
        language = group['language_id']
        if language in workers:
            reject('duplicate worker', language)
        workers[language] = group
        if group.get('worker_error'):
            reject('worker error: ' + group['worker_error'], language)
        if group.get('native_library_sha256') != native_sha:
            reject('worker native library identity missing or differs from parent', language)
        for row in group['cases']:
            identity = row['id']
            if identity in observed or identity not in current or row.get('language_id') != language:
                reject('unexpected, duplicate or misrouted observation', language, identity)
            observed[identity] = row
            if row['status'] in {'runtime_error', 'not_executed'}:
                reject('execution status: ' + row['status'], language, identity)
    for language in {c['language_id'] for c in cases} - workers.keys():
        reject('missing worker', language)
    for identity, case in current.items():
        if identity not in observed:
            reject('missing case observation', case['language_id'], identity)
    for identity, old in previous.items():
        language = old['language_id']; case = current.get(identity); row = observed.get(identity)
        if case is None:
            reject('baseline fixture missing; explicit baseline rebase required', language, identity)
        else:
            inputs = {'old_sha256': sha(case['old'].encode()), 'new_sha256': sha(case['new'].encode()),
                      'expected': case['expected'], 'language_id': case['language_id']}
            for key, value in inputs.items():
                if old.get(key) != value:
                    reject(f'baseline {key} changed; explicit baseline rebase required', language, identity)
                if row is not None and row.get(key) != value:
                    reject(f'observed {key} missing or differs from fixture', language, identity)
        if old['status'] == 'passed':
            if row is None or row.get('status') != 'passed' or row.get('oracle_evaluated') is not True:
                reject('previous pass did not execute and pass its oracle', language, identity)
        elif row is not None and row.get('status') == 'passed' and row.get('oracle_evaluated') is True:
            improvements.append(identity)
    return {'scope_languages': sorted(selected) if selected is not None else 'all baseline languages',
            'protected_passes': sum(row['status'] == 'passed' for row in previous.values()),
            'regression_count': len(regressions), 'regressions': regressions,
            'improvements': sorted(improvements)}


def worker(request):
    transport, library = worker_transport(request.get('native_library'))
    _c_abi_call = transport._c_abi_call
    _NativeCAbiOperationFailure = transport._NativeCAbiOperationFailure
    parsers = request['parsers']; cases = request['cases']; rows = []
    options = {'language_hint': request['language_id'], 'strict_plugins': True}
    config = json.dumps(options)
    for case in cases:
        started = time.monotonic(); row = {'id': case['id'], 'language_id': case['language_id'],
            'category': case['category'], 'provenance_class': case['provenance']['class'],
            'old_sha256': sha(case['old'].encode()), 'new_sha256': sha(case['new'].encode()),
            'expected': case['expected'], 'related_issues': case.get('related_issues', [])}
        try:
            envelope = _c_abi_call('live_diff_contents', request['repo'], case['filename'],
                                   case['old'], case['new'], config, parsers)
            incomplete = all(case['expected'].get(k) is None for k in
                ('has_semantic_changes', 'is_style_only', 'changes_empty'))
            if isinstance(envelope, dict) and isinstance(envelope.get('fallback'), str) and envelope['fallback'] and 'diff' not in envelope:
                row.update(status='bounded_explicit_fallback' if incomplete else 'failed',
                    oracle_evaluated=False, explicit_fallback=envelope,
                    classification_errors=[] if incomplete else ['valid-source review returned explicit fallback'])
                row['elapsed_ms'] = round((time.monotonic() - started) * 1000, 3)
                rows.append(row)
                print(json.dumps({'type': 'case', 'row': row}, ensure_ascii=False), flush=True)
                continue
            if not isinstance(envelope, dict) or not isinstance(envelope.get('diff'), dict):
                raise ValueError(f'missing review envelope: {envelope}')
            review = envelope['diff']; row['review'] = review
            row['classification_errors'] = check_classification(case, review)
            row['parse_errors'] = review.get('parse_errors', [])
            row['review_errors'] = check_review_nodes(review, case['old'], case['new'])
            row['tree_errors'] = []; row['trees'] = {}
            row['span_relationship_observations'] = []
            for side in ('old', 'new'):
                parsed = _c_abi_call('parse_to_tree', case['filename'], case[side], config, parsers)
                if not isinstance(parsed, dict) or not isinstance(parsed.get('tree'), dict):
                    raise ValueError(f'{side}: missing parse tree: {parsed}')
                row['trees'][side] = parsed
                row['tree_errors'].extend(f'{side}: {e}' for e in check_tree(parsed['tree'], case[side]))
                row['span_relationship_observations'].extend(f'{side}: {e}' for e in
                    check_tree(parsed['tree'], case[side], containment=True) if 'outside parent' in e)
                if parsed.get('language') != case['language_id']:
                    row['tree_errors'].append(f"{side}: wrong tree language {parsed.get('language')}")
            repeat = _c_abi_call('live_diff_contents', request['repo'], case['filename'],
                                 case['old'], case['new'], config, parsers)
            row['deterministic'] = stable_review(review) == stable_review(repeat.get('diff'))
            row['oracle_evaluated'] = not incomplete
            row['status'] = case_status(case, row)
            if incomplete:
                row['handling'] = 'diagnostics_present' if row['parse_errors'] else 'accepted_without_diagnostics'
        except Exception as error:
            incomplete = all(case['expected'].get(k) is None for k in
                ('has_semantic_changes', 'is_style_only', 'changes_empty'))
            row['status'] = ('bounded_explicit_error' if isinstance(error, _NativeCAbiOperationFailure)
                             else 'invalid_result') if incomplete else 'runtime_error'
            row['oracle_evaluated'] = False
            row['error'] = f'{type(error).__name__}: {error}'
        row['elapsed_ms'] = round((time.monotonic() - started) * 1000, 3)
        rows.append(row)
        print(json.dumps({'type': 'case', 'row': row}, ensure_ascii=False), flush=True)
    return {'language_id': request['language_id'], 'options': options, 'completed_count': len(rows),
            'peak_rss_kib': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
            'native_library_path': str(library), 'native_library_sha256': sha(library.read_bytes())}


def recover_worker(text, language, cases, error=None):
    """Only complete per-case observations count as executions after worker loss."""
    if isinstance(text, bytes):
        text = text.decode('utf-8', errors='replace')
    observed = {}; summary = {}
    wanted = {c['id'] for c in cases}
    for line in (text or '').splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if item.get('type') == 'case':
            row = item.get('row', {})
            if row.get('id') not in wanted or row['id'] in observed:
                raise ValueError('worker reported an unexpected or duplicate case')
            observed[row['id']] = row
        elif item.get('type') == 'complete':
            summary = item['summary']
    if error is None and (not summary or len(observed) != len(cases)):
        error = 'worker exited without complete per-case observations'
    rows = [observed.get(c['id'], {'id': c['id'], 'language_id': language,
        'category': c['category'], 'provenance_class': c.get('provenance', {}).get('class', 'unknown'),
        'status': 'not_executed', 'error': str(error), 'oracle_evaluated': False}) for c in cases]
    return {**summary, 'language_id': language, 'cases': rows,
            'completed_count': len(observed), 'attempted_count': None if error else len(observed),
            'worker_error': str(error) if error else None}


def run_group(language, cases, args):
    request = {'language_id': language, 'cases': cases, 'parsers': str(args.parsers.resolve()),
               'repo': str(Path(__file__).resolve().parents[1])}
    if args.native_library is not None:
        request['native_library'] = str(args.native_library.resolve())
    started = time.monotonic()
    try:
        result = subprocess.run([sys.executable, str(Path(__file__).resolve()), '--worker'],
            input=json.dumps(request), text=True, encoding='utf-8', capture_output=True,
            timeout=args.timeout, env={**os.environ, 'RAYON_NUM_THREADS': '1'})
        error = f'worker exit {result.returncode}: {result.stderr[-3000:]}' if result.returncode else None
        group = recover_worker(result.stdout, language, cases, error)
        group['stderr'] = result.stderr[-5000:]
    except subprocess.TimeoutExpired as error:
        group = recover_worker(error.stdout, language, cases, str(error))
    except (OSError, RuntimeError, ValueError) as error:
        group = recover_worker('', language, cases, str(error))
    group['wall_seconds'] = round(time.monotonic() - started, 3)
    return group


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--worker', action='store_true')
    parser.add_argument('--fixtures', type=Path)
    parser.add_argument('--parsers', type=Path)
    parser.add_argument('--report', type=Path)
    parser.add_argument('--languages', nargs='+')
    parser.add_argument('--native-library', type=Path, help='candidate Rust C ABI library (default: installed transport library)')
    parser.add_argument('--baseline', type=Path, help='reviewed assessment JSON; fail on regressions after retaining results')
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--timeout', type=int, default=120)
    args = parser.parse_args()
    if args.worker:
        summary = worker(json.load(sys.stdin))
        print(json.dumps({'type': 'complete', 'summary': summary}, ensure_ascii=False), flush=True); return
    if not all((args.fixtures, args.parsers, args.report)):
        parser.error('--fixtures, --parsers and --report are required')
    library = native_library_path(args.native_library)
    native_sha = sha(library.read_bytes())
    inventory_path = Path(__file__).resolve().parents[1] / 'tests/fixtures/external/inventory.json'
    inventory = json.loads(inventory_path.read_bytes())
    staged_raw = (args.parsers / 'parser_manifest.json').read_bytes(); staged = json.loads(staged_raw)
    if staged['parsers'] != inventory['parsers'] or staged['extension_index'] != inventory['extension_index']:
        raise ValueError('staged routing differs from fixed inventory')
    artifacts = {a['component']: a for a in staged['components']}
    for entry in inventory['parsers'].values():
        name = entry['wasm']
        if sha((args.parsers / name).read_bytes()) != artifacts[name]['sha256']:
            raise ValueError(f'component identity mismatch: {name}')
    cases, fixture_files = load_cases(args.fixtures.resolve(), inventory, args.languages)
    languages = sorted({c['language_id'] for c in cases}); groups = []
    raw_dir = args.report.parent / 'raw'; raw_dir.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        pending = {pool.submit(run_group, language, [c for c in cases if c['language_id'] == language], args): language for language in languages}
        for future in as_completed(pending):
            group = future.result(); groups.append(group)
            (raw_dir / (group['language_id'] + '.json')).write_text(json.dumps(group, indent=2, ensure_ascii=False) + '\n')
            print(group['language_id'], {s: sum(c['status'] == s for c in group['cases']) for s in sorted({c['status'] for c in group['cases']})}, flush=True)
    result = {'schema_version': 1, 'run_utc': datetime.now(timezone.utc).isoformat(),
        'scope': 'bounded diagnostic baseline; not full language certification',
        'interface': 'production Rust C ABI: live_diff_contents + parse_to_tree',
        'native_library_path': str(library), 'native_library_sha256': native_sha,
        'inventory_sha256': sha(inventory_path.read_bytes()), 'parser_manifest_sha256': sha(staged_raw),
        'fixture_files': fixture_files, 'parser_manifest': staged, 'languages': [],
        'limitations': ['Handwritten/repository probes are supplementary, not external gold.',
            'Range validity does not establish exact source-span accuracy.',
            'Parent-child span containment is observational; per-node name spans may legitimately exclude descendants.',
            'Correspondence/explanation precision-recall and exhaustive construct coverage not certified.',
            'Incomplete-input acceptance without diagnostics is an observation, not a correctness pass.',
            'RSS is Linux process peak; this is not sustained-load/resource-limit certification.']}
    for group in sorted(groups, key=lambda g: g['language_id']):
        rows = group['cases']; language = group['language_id']
        admitted = [c for c in cases if c['language_id'] == language and
                    any(c['expected'].get(k) is not None for k in ('has_semantic_changes', 'is_style_only', 'changes_empty'))]
        judged = [r for r in rows if r.get('oracle_evaluated')]
        result['languages'].append({'language_id': language, 'component': inventory['parsers'][language]['wasm'],
            'declared_cases': len(rows), 'executed': group['completed_count'],
            'attempted': group['attempted_count'], 'not_executed': sum(r['status'] == 'not_executed' for r in rows),
            'source_oracles_admitted': len(admitted), 'source_judged': len(judged),
            'passed': sum(r['status'] == 'passed' for r in judged),
            'failed': sum(r['status'] in {'failed', 'runtime_error', 'invalid_result'} for r in rows),
            'robustness_observations': sum(r['status'].startswith('bounded_') for r in rows),
            'classification_failures': sum(bool(r.get('classification_errors')) for r in rows),
            'range_validity_failures': sum(bool(r.get('tree_errors')) or any('group ' not in e for e in r.get('review_errors', [])) for r in rows),
            'group_reference_failures': sum(any('group ' in e for e in r.get('review_errors', [])) for r in rows),
            'source_execution_path': ('native Rust/tree-sitter after component selection' if language == 'python'
                else 'native Rust text-review; Wasm component for separate tree inspection' if language == 'generic'
                else 'selected Wasm component through production Rust host'),
            'span_relationship_observations': sum(bool(r.get('span_relationship_observations')) for r in rows),
            'external_cases': sum(r['provenance_class'] in {'reviewed_external_derived', 'upstream_gold'} for r in rows),
            'external_gold_cases': sum(r['provenance_class'] == 'upstream_gold' for r in rows),
            'wall_seconds': group['wall_seconds'], 'peak_rss_kib': group.get('peak_rss_kib'),
            'worker_error': group.get('worker_error'), 'raw_results': str(raw_dir / (language + '.json')),
            'native_library_path': group.get('native_library_path'),
            'native_library_sha256': group.get('native_library_sha256'),
            'case_results': [{k: v for k, v in row.items() if k not in {'review', 'trees'}} for row in rows]})
    if args.baseline is not None:
        baseline_raw = None
        try:
            baseline_raw = args.baseline.read_bytes()
            gate = compare_baseline(json.loads(baseline_raw), cases, groups, args.languages, native_sha)
        except (OSError, UnicodeError, ValueError) as error:
            message = f'invalid assessment baseline: {error}'
            gate = {'error': message, 'protected_passes': None, 'regression_count': 1,
                    'regressions': [{'language_id': '(baseline)', 'case_id': None, 'reason': message}],
                    'improvements': []}
        result['baseline_gate'] = {**gate, 'path': str(args.baseline),
                                   'sha256': sha(baseline_raw) if baseline_raw is not None else None}
    args.report.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    print(f"Report: {len(groups)} languages, {len(cases)} executions -> {args.report}")
    if args.baseline is not None:
        gate = result['baseline_gate']
        print(f"Baseline gate: {gate['regression_count']} regressions; {len(gate['improvements'])} improvements")
        for regression in gate['regressions']:
            print(f"  {regression['language_id']} {regression['case_id'] or '(worker)'}: {regression['reason']}")
        return 1 if gate['regression_count'] else 0
    return 0


if __name__ == '__main__':
    sys.exit(main())
