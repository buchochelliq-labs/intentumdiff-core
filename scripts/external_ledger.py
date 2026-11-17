#!/usr/bin/env python3
"""External-benchmark admission ledger (schema 2), validator and scorecard (#268).

Benchmark tooling only: no product semantics live here. The ledger records what
an independent oracle says about each case; IntentumDiff output is judged against
it and is never used to write an expectation. The scorecard reports every routed
language ID, with explicit gaps for languages without admitted evidence.

Commands::

    python scripts/external_ledger.py validate [--fixtures tests/fixtures/external]
    python scripts/external_ledger.py scorecard --results RUN.json --out-json X.json --out-md X.md

See docs/EXTERNAL_BENCHMARKS.md ("Admission ledger and scorecard (schema 2)").
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import sys

SCHEMA_VERSION = 2
CLASSES = ('upstream_gold', 'reviewed_external_derived', 'external_seed_only', 'handwritten_regression')
EXTERNAL_GOLD = frozenset({'upstream_gold', 'reviewed_external_derived'})
LANES = ('parser_fidelity', 'change_understanding', 'robustness_scaling')
BEHAVIOR_CLAIMS = ('changed', 'preserved', 'unknown')
ORACLE_BASES = ('executable_witness', 'curated_annotation', 'specification_review',
                'byte_identity', 'source_review', 'candidate_only')
ADMISSION = ('admitted', 'candidate', 'rejected')
CHANGE_TYPES = ('ADDITION', 'DELETION', 'MODIFICATION', 'REFACTORING', 'MOVE', 'REORDER')
REQUIRED_KINDS = ('change_fact', 'edit_region')
FORBIDDEN_KINDS = ('change_type', 'style_only', 'no_changes', 'fallback')
WITNESS_VERDICTS = ('supports_claim', 'contradicts_claim', 'inconclusive', 'blocked')
# Run-row statuses; `skipped` and `blocked` are never folded into pass/fail.
RUN_STATUSES = ('passed', 'failed', 'unsupported', 'skipped', 'blocked')
COUNT_KEYS = ('discovered', 'eligible', 'admitted', 'executed', 'passed', 'failed',
              'unsupported', 'skipped', 'blocked')
HEX40 = re.compile(r'[0-9a-f]{40}')
HEX64 = re.compile(r'[0-9a-f]{64}')


class LedgerError(ValueError):
    pass


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def encode(value) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True) + '\n').encode()


def _need(obj, key, kind, where):
    value = obj.get(key) if isinstance(obj, dict) else None
    if kind is str:
        ok = isinstance(value, str) and value != ''
    elif kind is bool:
        ok = type(value) is bool
    elif kind is int:
        ok = type(value) is int and value >= 0
    else:
        ok = isinstance(value, kind)
    if not ok:
        raise LedgerError(f'{where}: missing or invalid {key}')
    return value


def safe_path(root: Path, relative: str) -> Path:
    p = Path(relative)
    if not relative or p.is_absolute() or any(part in ('', '.', '..') for part in p.parts):
        raise LedgerError(f'unsafe path: {relative!r}')
    resolved = (root / p).resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise LedgerError(f'path escapes fixture root: {relative!r}')
    return resolved


def checked_file(root: Path, ref, where) -> bytes:
    path = _need(ref, 'path', str, where)
    digest = _need(ref, 'sha256', str, where)
    if not HEX64.fullmatch(digest):
        raise LedgerError(f'{where}: sha256 is not a 64-hex digest')
    try:
        data = safe_path(root, path).read_bytes()
    except OSError as error:
        raise LedgerError(f'{where}: unreadable {path}: {error}') from error
    if sha(data) != digest:
        raise LedgerError(f'{where}: sha256 mismatch for {path}')
    return data


def _validate_suite(root, suite, where):
    for key in ('suite_id', 'name', 'version', 'url', 'licence', 'attribution', 'evidence_class'):
        _need(suite, key, str, where)
    revision = _need(suite, 'revision', str, where)
    if suite.get('handwritten') is True:
        if revision != 'in-repository':
            raise LedgerError(f"{where}: handwritten suites use revision 'in-repository' (bytes pinned per file)")
    elif not HEX40.fullmatch(revision):
        raise LedgerError(f'{where}: revision must be an immutable 40-hex commit')
    acquisition = _need(suite, 'acquisition', dict, where)
    _need(acquisition, 'method', str, where + '.acquisition')
    for index, ref in enumerate(_need(acquisition, 'files', list, where + '.acquisition')):
        checked_file(root, ref, f'{where}.acquisition.files[{index}]')
    _need(acquisition, 'verification', list, where + '.acquisition')


def _validate_expected(expected, where):
    classification = _need(expected, 'classification', dict, where)
    for key in ('has_semantic_changes', 'is_style_only', 'changes_empty'):
        if key not in classification:
            raise LedgerError(f'{where}.classification: {key} must be present (null = unconstrained)')
        value = classification[key]
        if value is not None and type(value) is not bool:
            raise LedgerError(f'{where}.classification: {key} must be bool or null')
    if classification.get('completeness') not in (None, 'complete', 'equivalent', 'incomplete'):
        raise LedgerError(f'{where}.classification: invalid completeness')

    def requirements(items, label):
        for index, item in enumerate(items):
            here = f'{where}.{label}[{index}]'
            kind = _need(item, 'kind', str, here)
            if kind not in REQUIRED_KINDS:
                raise LedgerError(f'{here}: unknown required kind {kind}')
            types = item.get('change_types_any_of')
            if not isinstance(types, list) or not types or any(t not in CHANGE_TYPES for t in types):
                raise LedgerError(f'{here}: change_types_any_of must list known change types')
            if kind == 'change_fact':
                if not any(item.get(k) for k in ('old_label', 'new_label')):
                    raise LedgerError(f'{here}: change_fact needs old_label or new_label')
            else:
                for side in ('old_range', 'new_range'):
                    rng = _need(item, side, dict, here)
                    start, end = _need(rng, 'start_byte', int, here), _need(rng, 'end_byte', int, here)
                    if start > end:
                        raise LedgerError(f'{here}: reversed {side}')
            _need(item, 'evidence', str, here)

    requirements(_need(expected, 'required', list, where), 'required')
    for index, alternative in enumerate(_need(expected, 'alternatives', list, where)):
        _need(alternative, 'reason', str, f'{where}.alternatives[{index}]')
        requirements(_need(alternative, 'required', list, f'{where}.alternatives[{index}]'),
                     f'alternatives[{index}].required')
    for index, item in enumerate(_need(expected, 'forbidden', list, where)):
        here = f'{where}.forbidden[{index}]'
        kind = _need(item, 'kind', str, here)
        if kind not in FORBIDDEN_KINDS:
            raise LedgerError(f'{here}: unknown forbidden kind {kind}')
        if kind == 'change_type' and item.get('change_type') not in CHANGE_TYPES:
            raise LedgerError(f'{here}: unknown change_type')
        _need(item, 'reason', str, here)


def validate_case(root, case, suites, inventory, witness_dir_ok=True):
    where = f"case {case.get('id')!r}"
    _need(case, 'id', str, where)
    suite_id = _need(case, 'suite_id', str, where)
    if suite_id not in suites:
        raise LedgerError(f'{where}: unknown suite {suite_id}')
    language = _need(case, 'language_id', str, where)
    if language not in inventory['parsers']:
        raise LedgerError(f'{where}: unregistered language {language}')
    for key in ('dialect', 'filename', 'change_family', 'licence', 'attribution'):
        _need(case, key, str, where)
    cls = _need(case, 'class', str, where)
    if cls not in CLASSES:
        raise LedgerError(f'{where}: unknown class {cls}')
    if _need(case, 'lane', str, where) not in LANES:
        raise LedgerError(f'{where}: unknown lane')
    upstream = _need(case, 'upstream', dict, where)
    _need(upstream, 'case_id', str, where + '.upstream')
    if cls != 'handwritten_regression':
        checked_file(root, _need(upstream, 'record', dict, where + '.upstream'), where + '.upstream.record')
    old = checked_file(root, _need(case, 'old', dict, where), where + '.old')
    new = checked_file(root, _need(case, 'new', dict, where), where + '.new')
    derivation = _need(case, 'derivation', list, where)
    if not derivation or any(not isinstance(s, dict) or not s.get('step') for s in derivation):
        raise LedgerError(f'{where}: derivation history must list at least one step')
    behavior = _need(case, 'behavior', dict, where)
    claim = _need(behavior, 'claim', str, where + '.behavior')
    if claim not in BEHAVIOR_CLAIMS:
        raise LedgerError(f'{where}: unknown behavior claim')
    _need(behavior, 'assumptions', str, where + '.behavior')
    oracle = _need(case, 'oracle', dict, where)
    basis = _need(oracle, 'basis', str, where + '.oracle')
    if basis not in ORACLE_BASES:
        raise LedgerError(f'{where}: unknown oracle basis {basis}')
    _need(oracle, 'notes', str, where + '.oracle')
    admission = _need(case, 'admission', dict, where)
    status = _need(admission, 'status', str, where + '.admission')
    if status not in ADMISSION:
        raise LedgerError(f'{where}: unknown admission status')
    _need(admission, 'reason', str, where + '.admission')
    witness = oracle.get('witness')
    if witness is not None:
        data = checked_file(root, witness, where + '.oracle.witness')
        record = json.loads(data)
        if record.get('case_id') != case['id'] or record.get('old_sha256') != case['old']['sha256'] \
                or record.get('new_sha256') != case['new']['sha256']:
            raise LedgerError(f'{where}: witness record is for different bytes')
        if record.get('verdict') not in WITNESS_VERDICTS:
            raise LedgerError(f'{where}: witness verdict unknown')
    # Class rules (#268): an admitted external-gold case needs an executed witness that
    # supports its behavior claim. Byte identity alone does not promote a case.
    if cls in EXTERNAL_GOLD and status == 'admitted':
        if basis != 'executable_witness' or witness is None \
                or json.loads(checked_file(root, witness, where))['verdict'] != 'supports_claim':
            raise LedgerError(f'{where}: {cls} requires an executed witness supporting the claim')
    if cls in EXTERNAL_GOLD and suites[suite_id].get('handwritten'):
        raise LedgerError(f'{where}: a handwritten suite cannot supply {cls} cases')
    if cls == 'handwritten_regression' and not suites[suite_id].get('handwritten'):
        raise LedgerError(f'{where}: handwritten_regression cases belong to a handwritten suite')
    if cls == 'upstream_gold' and derivation[0].get('step') != 'verbatim_upstream':
        raise LedgerError(f'{where}: upstream_gold bytes must be verbatim upstream (derived cases are reviewed_external_derived)')
    if (basis == 'byte_identity' or oracle.get('byte_identity')) and old != new:
        raise LedgerError(f'{where}: byte identity claimed for different bytes')
    if cls == 'external_seed_only' and status == 'admitted':
        raise LedgerError(f'{where}: seed-only cases cannot be admitted')
    if basis == 'candidate_only' and status == 'admitted':
        raise LedgerError(f'{where}: candidate-only oracle cannot admit a case')
    if status == 'admitted':
        expected = _need(case, 'expected', dict, where)
        _validate_expected(expected, where + '.expected')
        for item in expected['required']:
            if item['kind'] == 'edit_region':
                for side, data in (('old_range', old), ('new_range', new)):
                    if item[side]['end_byte'] > len(data):
                        raise LedgerError(f'{where}: {side} outside source')
        if claim == 'unknown':
            raise LedgerError(f'{where}: an admitted case needs a changed/preserved behavior claim')
    elif case.get('expected') is not None:
        raise LedgerError(f'{where}: unadmitted cases must not carry expectations (expected: null)')
    return case


def validate(root: Path, ledger=None, inventory=None):
    """Validate the complete ledger; returns (ledger, inventory)."""
    root = root.resolve()
    ledger_path = root / 'admission' / 'ledger.json'
    if ledger is None:
        ledger = json.loads(ledger_path.read_bytes())
    if ledger.get('schema_version') != SCHEMA_VERSION:
        raise LedgerError('ledger schema_version 2 required')
    inv_ref = _need(ledger, 'inventory', dict, 'ledger')
    inv_bytes = checked_file(root, inv_ref, 'ledger.inventory')
    if inventory is None:
        inventory = json.loads(inv_bytes)
    if not inventory.get('parsers'):
        raise LedgerError('inventory has no registered languages')
    selection = checked_file(root, _need(ledger, 'language_selection', dict, 'ledger'), 'ledger.language_selection')
    selected = {row['language_id'] for row in json.loads(selection)['languages']}
    if selected != set(inventory['parsers']):
        raise LedgerError('language selection ledger must cover exactly the registered languages')
    suites = {}
    for index, suite in enumerate(_need(ledger, 'suites', list, 'ledger')):
        _validate_suite(root, suite, f'suite[{index}]')
        if suite['suite_id'] in suites:
            raise LedgerError(f"duplicate suite {suite['suite_id']}")
        suites[suite['suite_id']] = suite
    seen = set()
    for index, row in enumerate(_need(ledger, 'discovery', list, 'ledger')):
        where = f'discovery[{index}]'
        if row.get('suite_id') not in suites or row.get('language_id') not in inventory['parsers'] \
                or row.get('lane') not in LANES:
            raise LedgerError(f'{where}: unknown suite/language/lane')
        key = (row['suite_id'], row['language_id'], row['lane'])
        if key in seen:
            raise LedgerError(f'{where}: duplicate discovery row')
        seen.add(key)
        discovered, eligible = _need(row, 'discovered', int, where), _need(row, 'eligible', int, where)
        if eligible > discovered:
            raise LedgerError(f'{where}: eligible exceeds discovered')
        _need(row, 'eligibility_rule', str, where)
    ids = set()
    for case in _need(ledger, 'cases', list, 'ledger'):
        validate_case(root, case, suites, inventory)
        if case['id'] in ids:
            raise LedgerError(f"duplicate case id {case['id']}")
        ids.add(case['id'])
    admitted = Counter((c['suite_id'], c['language_id'], c['lane']) for c in ledger['cases']
                       if c['admission']['status'] == 'admitted' and c['class'] in EXTERNAL_GOLD)
    for key, count in admitted.items():
        row = next((r for r in ledger['discovery'] if (r['suite_id'], r['language_id'], r['lane']) == key), None)
        if row is None:
            raise LedgerError(f'admitted external cases without a discovery row: {key}')
    return ledger, inventory


# --- judging -------------------------------------------------------------------------

def _offset(source: bytes, line: int, col: int):
    lines = source.split(b'\n')
    if line >= len(lines) or col > len(lines[line]):
        return None
    return sum(len(text) + 1 for text in lines[:line]) + col


def node_range(node, source: bytes):
    p = node.get('position') if isinstance(node, dict) else None
    if not isinstance(p, dict):
        return None
    try:
        start = _offset(source, p['start_line'], p['start_col'])
        end = _offset(source, p['end_line'], p['end_col'])
    except (KeyError, TypeError):
        return None
    return None if start is None or end is None or start > end else (start, end)


def _touches(rng, region):
    return rng is not None and rng[0] <= region['end_byte'] and region['start_byte'] <= rng[1]


def _labels(node):
    out = []
    stack = [node]
    while stack:
        n = stack.pop()
        if isinstance(n, dict):
            if isinstance(n.get('label'), str):
                out.append(n['label'])
            stack.extend(n.get('children') or [])
    return out


def satisfies(item, change, old: bytes, new: bytes):
    if change.get('change_type') not in item['change_types_any_of']:
        return False
    if item['kind'] == 'edit_region':
        old_hit = _touches(node_range(change.get('old_node'), old), item['old_range'])
        new_hit = _touches(node_range(change.get('new_node'), new), item['new_range'])
        return old_hit or new_hit
    for side in ('old', 'new'):
        wanted = item.get(side + '_label')
        if wanted is None:
            continue
        node = change.get(side + '_node')
        if node is None:
            return False
        if item.get('match') == 'descendant_evidence':
            if wanted not in _labels(node):
                return False
        elif node.get('label') != wanted:
            return False
    return True


def judge(case, review, old: bytes, new: bytes):
    """Return independent-oracle errors for one review; [] means the oracle is met."""
    expected = case['expected']; errors = []
    changes = review.get('changes') if isinstance(review.get('changes'), list) else []
    for key, wanted in expected['classification'].items():
        if key == 'completeness':
            if wanted is not None and review.get('completeness') != wanted:
                errors.append(f"completeness: expected {wanted}, got {review.get('completeness')}")
            continue
        actual = (not changes) if key == 'changes_empty' else review.get(key)
        if wanted is not None and actual != wanted:
            errors.append(f'{key}: expected {wanted}, got {actual}')

    def unmet(requirements):
        return [item for item in requirements
                if not any(satisfies(item, change, old, new) for change in changes if isinstance(change, dict))]

    missing = unmet(expected['required'])
    if missing and not any(not unmet(alt['required']) for alt in expected['alternatives']):
        errors.extend(f"missing required {m['kind']} ({'/'.join(m['change_types_any_of'])}): {m['evidence']}"
                      for m in missing)
    for item in expected['forbidden']:
        kind = item['kind']
        if kind == 'change_type' and any(c.get('change_type') == item['change_type'] for c in changes
                                         if isinstance(c, dict)):
            errors.append(f"forbidden {item['change_type']} reported: {item['reason']}")
        elif kind == 'style_only' and review.get('is_style_only') is True:
            errors.append(f"forbidden style-only classification: {item['reason']}")
        elif kind == 'no_changes' and not changes:
            errors.append(f"forbidden empty change list: {item['reason']}")
        elif kind == 'fallback' and review.get('is_fallback') is not False:
            errors.append(f"forbidden fallback: {item['reason']}")
    return errors


# --- scorecard ------------------------------------------------------------------------

def _rate(numerator, denominator, reason):
    if not denominator:
        return {'value': None, 'numerator': numerator, 'denominator': 0, 'na_reason': reason}
    return {'value': round(numerator / denominator, 4), 'numerator': numerator, 'denominator': denominator}


def _blank():
    return {key: 0 for key in COUNT_KEYS}


def build_scorecard(ledger, inventory, selection, results):
    """Combine ledger + run results into per-language rows. Every language gets rows."""
    selection = {row['language_id']: row for row in selection['languages']}
    suites = {s['suite_id']: s for s in ledger['suites']}
    by_case = {row['case_id']: row for row in (results or {}).get('cases', [])}
    if results is not None:
        unknown = set(by_case) - {c['id'] for c in ledger['cases']}
        if unknown:
            raise LedgerError(f'results contain unknown cases: {sorted(unknown)}')
    rows = defaultdict(_blank)
    detail = defaultdict(lambda: {'changed': [0, 0], 'preserved': [0, 0], 'failures': []})
    # Discovery counts upstream tasks (the original suite inventory) on the upstream_gold row.
    # Every non-upstream case was created by derivation/authoring, so it counts itself.
    for d in ledger['discovery']:
        row = rows[(d['language_id'], d['suite_id'], 'upstream_gold', d['lane'])]
        row['discovered'] = d['discovered']; row['eligible'] = d['eligible']
    for case in ledger['cases']:
        key = (case['language_id'], case['suite_id'], case['class'], case['lane'])
        row = rows[key]
        if case['class'] != 'upstream_gold':
            row['discovered'] += 1
            row['eligible'] += case['admission']['status'] != 'rejected'
        if case['admission']['status'] != 'admitted':
            continue
        row['admitted'] += 1
        observed = by_case.get(case['id'])
        status = 'skipped' if observed is None else observed['status']
        if status not in RUN_STATUSES:
            raise LedgerError(f"invalid run status {status} for {case['id']}")
        row[status] += 1
        if status in ('passed', 'failed'):
            row['executed'] += 1
            bucket = detail[key][case['behavior']['claim']]
            bucket[1] += 1
            bucket[0] += status == 'passed'
            if status == 'failed':
                detail[key]['failures'].append({'case_id': case['id'], 'errors': observed.get('errors', []),
                                                'known_issue': case.get('known_issue')})
    out = []
    for language in sorted(inventory['parsers']):
        lang_rows = sorted(k for k in rows if k[0] == language)
        plan = selection.get(language, {})
        entry = {'language_id': language, 'component': inventory['parsers'][language].get('wasm'),
                 'component_sha256': (results or {}).get('components', {}).get(inventory['parsers'][language].get('wasm')),
                 'selected_primary_source': plan.get('source_url'),
                 'selected_evidence_class': plan.get('evidence_class'), 'rows': [], 'lanes': {}}
        for key in lang_rows:
            counts = rows[key]; d = detail[key]
            entry['rows'].append({
                'suite_id': key[1], 'suite_revision': suites[key[1]]['revision'],
                'suite_version': suites[key[1]]['version'], 'class': key[2], 'lane': key[3],
                'counts': dict(counts),
                'pass_rate': _rate(counts['passed'], counts['executed'],
                                   'no admitted case was executed' if counts['admitted'] else 'no admitted cases'),
                'meaningful_change_recall': _rate(d['changed'][0], d['changed'][1],
                                                  'no executed behavior-changing cases'),
                'preservation_false_alarm_rate': _rate(d['preserved'][1] - d['preserved'][0], d['preserved'][1],
                                                       'no executed preservation controls'),
                'failures': d['failures']})
        for lane in LANES:
            lane_rows = [r for r in entry['rows'] if r['lane'] == lane and r['class'] in EXTERNAL_GOLD]
            executed = sum(r['counts']['executed'] for r in lane_rows)
            passed = sum(r['counts']['passed'] for r in lane_rows)
            if executed:
                status = 'measured'
                reason = None
            elif any(r['counts']['blocked'] for r in lane_rows):
                status, reason = 'blocked', 'admitted external cases could not execute (see rows)'
            else:
                status = 'gap'
                reason = ('no admitted external case executed in this lane; selection: '
                          + (plan.get('reason') or 'none recorded'))
            entry['lanes'][lane] = {'status': status, 'external_gold_pass_rate':
                                    _rate(passed, executed, reason or ''), 'na_reason': reason}
        out.append(entry)
    gaps = sum(all(l['status'] != 'measured' for l in e['lanes'].values()) for e in out)
    return {'schema_version': 1, 'contract': 'core#268 per-language external-benchmark scorecard',
            'ledger_sha256': None, 'languages': out,
            'summary': {'registered_languages': len(out), 'languages_without_measured_external_lane': gaps,
                        'pooled_summary': 'none: per-language rows only; no macro average is claimed'}}


def _fmt_rate(rate):
    if rate['value'] is None:
        return f"N/A (0 of 0: {rate['na_reason']})" if rate.get('na_reason') else 'N/A'
    return f"{rate['numerator']}/{rate['denominator']} ({rate['value']:.0%})"


def render_markdown(card, header_lines):
    lines = list(header_lines)
    run = card.get('run') or {}
    summary = card['summary']
    measured = summary['registered_languages'] - summary['languages_without_measured_external_lane']
    lines += ['', '## Identity', '',
              f"- Ledger SHA-256: `{card.get('ledger_sha256')}`",
              f"- Core commit: `{run.get('core_commit')}` (crates dirty: {run.get('core_crates_dirty')})",
              f"- C ABI library SHA-256: `{run.get('native_library_sha256')}`",
              f"- Staged parser manifest SHA-256: `{run.get('parser_manifest_sha256')}`",
              f"- Interface: {run.get('interface') or 'not executed (no runner report)'}",
              f"- Run (UTC): {run.get('run_utc')}",
              '', '## Summary', '',
              f"- Registered language IDs: {summary['registered_languages']}.",
              f"- With at least one measured external lane: {measured}; gaps: "
              f"{summary['languages_without_measured_external_lane']}.",
              '- Handwritten regressions are shown in their own rows and never count toward a lane.',
              '', '## Per-language lane status (all registered IDs)', '',
              '| Language | Change understanding | Parser fidelity | Robustness/scaling | Selected primary source (evidence class) |',
              '|---|---|---|---|---|']
    for e in card['languages']:
        cells = []
        for lane in ('change_understanding', 'parser_fidelity', 'robustness_scaling'):
            l = e['lanes'][lane]
            cells.append(f"{_fmt_rate(l['external_gold_pass_rate'])}" if l['status'] == 'measured'
                         else l['status'].upper())
        source = e['selected_primary_source']
        lines.append(f"| `{e['language_id']}` | {' | '.join(cells)} | "
                     f"{('<' + source + '>') if source else '—'} ({e['selected_evidence_class'] or 'none'}) |")
    lines += ['', '## Rows with cases (language × suite × class × lane)', '',
              '| Language | Suite | Class | Lane | Disc. | Elig. | Adm. | Exec. | Pass | Fail | Unsup. | Skip | Blocked | Pass rate | Change recall | Preservation false alarms |',
              '|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|---|']
    for e in card['languages']:
        for r in e['rows']:
            c = r['counts']
            lines.append(f"| `{e['language_id']}` | {r['suite_id']} | {r['class']} | {r['lane']} | "
                         + ' | '.join(str(c[k]) for k in COUNT_KEYS)
                         + f" | {_fmt_rate(r['pass_rate'])} | {_fmt_rate(r['meaningful_change_recall'])}"
                         f" | {_fmt_rate(r['preservation_false_alarm_rate'])} |")
    failures = [(e['language_id'], f) for e in card['languages'] for r in e['rows'] for f in r['failures']]
    lines += ['', '## Failed admitted cases', '']
    if not failures:
        lines.append('None.')
    for language, f in failures:
        issue = f" — {f['known_issue']}" if f.get('known_issue') else ''
        lines.append(f"- `{language}` `{f['case_id']}`{issue}: " + '; '.join(f['errors'][:3]))
    return '\n'.join(lines) + '\n'


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    v = sub.add_parser('validate')
    v.add_argument('--fixtures', type=Path, default=Path('tests/fixtures/external'))
    s = sub.add_parser('scorecard')
    s.add_argument('--fixtures', type=Path, default=Path('tests/fixtures/external'))
    s.add_argument('--results', type=Path, help='runner results JSON (omit: every admitted case is skipped)')
    s.add_argument('--out-json', type=Path, required=True)
    s.add_argument('--out-md', type=Path, required=True)
    s.add_argument('--title', default='External-benchmark scorecard')
    args = parser.parse_args(argv)
    try:
        ledger, inventory = validate(args.fixtures)
    except (LedgerError, OSError, ValueError) as error:
        print(f'ledger invalid: {error}', file=sys.stderr)
        return 2
    if args.command == 'validate':
        admitted = Counter((c['class'], c['admission']['status']) for c in ledger['cases'])
        print(f"ledger valid: {len(ledger['cases'])} cases; " +
              ', '.join(f'{k[0]}/{k[1]}={n}' for k, n in sorted(admitted.items())))
        return 0
    root = args.fixtures.resolve()
    selection = json.loads((root / ledger['language_selection']['path']).read_bytes())
    results = json.loads(args.results.read_bytes()) if args.results else None
    card = build_scorecard(ledger, inventory, selection, results)
    card['ledger_sha256'] = sha((root / 'admission' / 'ledger.json').read_bytes())
    card['run'] = {k: v for k, v in (results or {}).items() if k != 'cases'} or None
    args.out_json.write_bytes(encode(card))
    header = [f'# {args.title}', '',
              'Generated by `scripts/external_ledger.py scorecard` from the schema-2 admission ledger and a runner report.',
              'Rates carry their denominators; N/A means zero cases, never 0% or 100%. GAP means no admitted external',
              'case was executed in that lane; it is not a pass. No pooled or macro average is claimed.']
    args.out_md.write_text(render_markdown(card, header))
    return 0


if __name__ == '__main__':
    sys.exit(main())
