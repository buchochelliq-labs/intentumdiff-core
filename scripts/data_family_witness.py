#!/usr/bin/env python3
"""Execute reference witnesses for the #262 SQL and data-platform family (#268 admission).

Benchmark tooling outside the runtime engine. It establishes the external oracle for a
case, never IntentumDiff's answer, by running the exact admitted bytes on real database
engines or on the language owner's own parser. IntentumDiff is not imported or consulted.

Subcommands::

    # PostgreSQL regression file: pg_regress-equivalent transcript vs upstream expected
    # output, plus the server's own statement inventory (csvlog, log_statement=all).
    python scripts/sql_family_witness.py pg-file --case-id ID --source F.sql \
        [--expected F.out] --csvlog /path/pg.csv --out W.json

    # Before/after pair: run both programs on each engine and compare the result set of
    # the final statement (ordered when the query defines a total order, else as a bag).
    python scripts/sql_family_witness.py pair --case-id ID --old A.sql --new B.sql \
        --claim changed|preserved --order ordered|bag [--engines postgresql,sqlite] --out W.json

    # T-SQL: Microsoft ScriptDom (scripts/witness_tools/scriptdom, built with dotnet).
    python scripts/data_family_witness.py scriptdom --case-id ID --old A.sql [--new B.sql] \
        [--expect-errors150 N] --tool ScriptDomWitness.dll --out W.json

    # Power Query M: @microsoft/powerquery-parser (scripts/witness_tools/powerquery).
    python scripts/data_family_witness.py powerquery --case-id ID --source F.pq \
        --node-path DIR/node_modules --out W.json

    # ADF pipeline JSON: strict JSON value comparison and the documented activity
    # dependency-condition semantics evaluated over a predecessor-outcome matrix.
    python scripts/data_family_witness.py json-pair --case-id ID --old A.json --new B.json \
        --claim changed|preserved --out W.json

PostgreSQL is reached through the usual PGHOST/PGPORT/PGUSER environment and the server
must run with ``logging_collector=on``, ``log_destination=csvlog``, ``log_statement=all``
and ``log_min_error_statement=error`` for ``pg-file``. SQLite is Python's ``sqlite3``.
A missing engine yields verdict ``blocked``; an engine error or a disagreement between
engines yields ``inconclusive``. Neither admits a case.

Same result on one fixture is bounded evidence, not program equivalence: a
``preserved`` claim must also rest on a transformation that is equivalent by the SQL
semantics stated in the ledger; the witness only confirms it on the upstream data.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import uuid

TIMEOUT = 120
PG_ENV = {'PGTZ': 'America/Los_Angeles', 'PGDATESTYLE': 'Postgres, MDY', 'LC_MESSAGES': 'C'}
PG_DB_SETTINGS = ("lc_messages TO 'C'", "lc_monetary TO 'C'", "lc_numeric TO 'C'", "lc_time TO 'C'",
                  "bytea_output TO 'hex'", "timezone_abbreviations TO 'Default'")


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _psql(args, *, stdin=None, env=None, merge=False):
    return subprocess.run(['psql', '-X', *args], input=stdin, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT if merge else subprocess.PIPE,
                          timeout=TIMEOUT, env={**os.environ, **(env or {})})


def pg_version():
    if shutil.which('psql') is None:
        return None
    out = _psql(['-At', '-d', 'postgres', '-c', 'select version()'])
    return out.stdout.strip() if out.returncode == 0 else None


def pg_fresh_db(name):
    """Fresh database with the pg_regress database settings."""
    for sql in (f'DROP DATABASE IF EXISTS "{name}"', f'CREATE DATABASE "{name}" TEMPLATE template0'):
        out = _psql(['-q', '-d', 'postgres', '-c', sql])
        if out.returncode:
            raise RuntimeError(out.stderr)
    for setting in PG_DB_SETTINGS:
        _psql(['-q', '-d', 'postgres', '-c', f'ALTER DATABASE "{name}" SET {setting}'])


def pg_drop_db(name):
    _psql(['-q', '-d', 'postgres', '-c', f'DROP DATABASE IF EXISTS "{name}"'])


def pg_statement_inventory(csvlog: Path, application: str, source: bytes):
    """Statements the server received from this run, located in the exact source bytes.

    psql (PostgreSQL's own lexer) splits the script; the server logs each statement it
    receives (``statement:``) or, for a statement it could not even parse, the ERROR row
    with the statement text. Each text is located in order in the source to recover its
    byte offset; an unlocated statement is reported rather than guessed.
    """
    statements = []
    with csvlog.open(newline='', encoding='utf-8') as handle:
        for row in csv.reader(handle):
            if len(row) < 23 or row[22] != application:
                continue
            severity, state, message, query = row[11], row[12], row[13], row[19]
            if severity == 'LOG' and message.startswith('statement: '):
                statements.append({'text': message[len('statement: '):], 'syntax_error': False})
            elif severity == 'ERROR' and state == '42601':
                statements.append({'text': query, 'syntax_error': True, 'message': message})
    text = source.decode('utf-8'); cursor = 0; located = []
    for item in statements:
        index = text.find(item['text'], cursor)
        if index < 0:
            located.append({**item, 'start_byte': None, 'end_byte': None})
            continue
        start = len(text[:index].encode()); end = start + len(item['text'].encode())
        located.append({**item, 'start_byte': start, 'end_byte': end})
        cursor = index + len(item['text'])
    return located


def pg_file(args):
    source = args.source.read_bytes()
    version = pg_version()
    record = {'schema_version': 1, 'case_id': args.case_id, 'old_sha256': sha(source), 'new_sha256': sha(source),
              'behavior_claim': 'preserved', 'oracle': 'postgresql_regress_transcript_and_statement_inventory',
              'engines': [{'name': 'postgresql', 'version': version}]}
    if version is None:
        return {**record, 'status': 'blocked', 'verdict': 'blocked', 'reason': 'psql/PostgreSQL unavailable'}
    tag = uuid.uuid4().hex[:12]; db = f'w262_{tag}'; application = f'w262/{args.case_id}/{tag}'
    pg_fresh_db(db)
    try:
        out = _psql(['-a', '-q', '-d', db, '-v', 'HIDE_TABLEAM=on', '-v', 'HIDE_TOAST_COMPRESSION=on'],
                    stdin=source.decode('utf-8'), env={**PG_ENV, 'PGAPPNAME': application}, merge=True)
    finally:
        pg_drop_db(db)
    transcript = out.stdout.encode()  # stdout+stderr interleaved, as pg_regress captures it
    record['settings'] = {'env': PG_ENV, 'database': list(PG_DB_SETTINGS),
                          'psql': ['-X', '-a', '-q', '-v', 'HIDE_TABLEAM=on', '-v', 'HIDE_TOAST_COMPRESSION=on']}
    record['transcript_sha256'] = sha(transcript)
    if args.expected is not None:
        expected = args.expected.read_bytes()
        record['expected_sha256'] = sha(expected)
        record['transcript_matches_expected'] = transcript == expected
    inventory = pg_statement_inventory(args.csvlog, application, source)
    record['statement_count'] = len(inventory)
    record['syntax_error_count'] = sum(s['syntax_error'] for s in inventory)
    record['unlocated_count'] = sum(s['start_byte'] is None for s in inventory)
    record['statements'] = [{k: s[k] for k in ('start_byte', 'end_byte', 'syntax_error')} for s in inventory]
    record['status'] = 'executed'
    ok = (record['statement_count'] > 0 and record['syntax_error_count'] == 0 and record['unlocated_count'] == 0
          and record.get('transcript_matches_expected', True))
    record['verdict'] = 'supports_claim' if ok else 'inconclusive'
    record['claim_tested'] = ('every statement is accepted by the PostgreSQL parser (no SQLSTATE 42601), each '
                              'statement is located in the exact bytes'
                              + (', and the pg_regress transcript equals the upstream expected output'
                                 if args.expected is not None else ''))
    return record


# --- before/after pairs ------------------------------------------------------------------

def split_sqlite(text):
    statements, buffer = [], ''
    for line in text.splitlines(keepends=True):
        buffer += line
        if sqlite3.complete_statement(buffer):
            if buffer.strip():
                statements.append(buffer)
            buffer = ''
    if buffer.strip() and not all(l.strip().startswith('--') or not l.strip() for l in buffer.splitlines()):
        statements.append(buffer)
    return statements


def run_sqlite(text):
    db = sqlite3.connect(':memory:')
    try:
        rows = None
        for statement in split_sqlite(text):
            cursor = db.execute(statement)
            if cursor.description is not None:
                rows = [list(r) for r in cursor.fetchall()]
        return {'outcome': 'ok', 'rows': rows}
    except sqlite3.Error as error:
        return {'outcome': 'error', 'error': f'{type(error).__name__}: {error}'}
    finally:
        db.close()


def run_postgresql(text):
    tag = uuid.uuid4().hex[:12]; db = f'w262_{tag}'
    pg_fresh_db(db)
    try:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'prog.sql'; path.write_text(text, encoding='utf-8')
            out = _psql(['-q', '-A', '-t', '-F', '\x1f', '-v', 'ON_ERROR_STOP=1', '-d', db, '-f', str(path)],
                        env=PG_ENV)
    finally:
        pg_drop_db(db)
    if out.returncode:
        return {'outcome': 'error', 'error': out.stderr.strip()[-500:]}
    # Only SELECT output reaches stdout in quiet tuples-only mode; the program's final
    # statement is its only query, so stdout is exactly its result set.
    rows = [line.split('\x1f') for line in out.stdout.splitlines() if line != '']
    return {'outcome': 'ok', 'rows': rows}


ENGINES = {'postgresql': (pg_version, run_postgresql),
           'sqlite': (lambda: f'SQLite {sqlite3.sqlite_version} (Python sqlite3 module)', run_sqlite)}


def _canonical(rows, order):
    rows = [[None if v is None else str(v) for v in r] for r in rows]
    return rows if order == 'ordered' else sorted(rows, key=lambda r: json.dumps(r))


def pair(args):
    old, new = args.old.read_bytes(), args.new.read_bytes()
    record = {'schema_version': 1, 'case_id': args.case_id, 'old_sha256': sha(old), 'new_sha256': sha(new),
              'behavior_claim': args.claim, 'oracle': 'final_statement_result_set_comparison',
              'comparison': {'order': args.order, 'values': 'text rendering per engine; NULL distinct from empty'},
              'engines': [], 'results': {}}
    conclusions = []
    for name in args.engines:
        version_fn, run = ENGINES[name]
        version = version_fn()
        record['engines'].append({'name': name, 'version': version})
        if version is None:
            record['results'][name] = {'outcome': 'blocked'}
            conclusions.append('blocked'); continue
        a, b = run(old.decode('utf-8')), run(new.decode('utf-8'))
        entry = {'old': a, 'new': b}
        if a['outcome'] != 'ok' or b['outcome'] != 'ok' or a['rows'] is None or b['rows'] is None:
            entry['conclusion'] = 'error'
        else:
            ca, cb = _canonical(a['rows'], args.order), _canonical(b['rows'], args.order)
            entry['old_rows_sha256'] = sha(json.dumps(ca).encode()); entry['new_rows_sha256'] = sha(json.dumps(cb).encode())
            entry['conclusion'] = 'same' if ca == cb else 'different'
        record['results'][name] = entry
        conclusions.append(entry['conclusion'])
    wanted = 'different' if args.claim == 'changed' else 'same'
    record['status'] = 'executed' if any(c in ('same', 'different') for c in conclusions) else 'blocked'
    if 'blocked' in conclusions and len(set(conclusions)) == 1:
        record['verdict'] = 'blocked'
    elif all(c == wanted for c in conclusions):
        record['verdict'] = 'supports_claim'
    elif all(c in ('same', 'different') for c in conclusions) and all(c != wanted for c in conclusions):
        record['verdict'] = 'contradicts_claim'
    else:
        record['verdict'] = 'inconclusive'
    return record


# --- T-SQL: Microsoft ScriptDom ----------------------------------------------------------

def _utf16_to_byte(text: str, offset: int, prefix: int) -> int:
    """UTF-16 code-unit offset into BOM-stripped text -> byte offset into the file."""
    return prefix + len(text.encode('utf-16-le')[:2 * offset].decode('utf-16-le').encode())


def _decoded(data: bytes):
    bom = data.startswith(b'\xef\xbb\xbf')
    return data[3:].decode('utf-8') if bom else data.decode('utf-8'), 3 if bom else 0


def _statements_from_offsets(data, offsets):
    text, prefix = _decoded(data)
    return [_utf16_to_byte(text, o, prefix) for o in offsets]


def scriptdom(args):
    old = args.old.read_bytes(); new = (args.new or args.old).read_bytes()
    claim = 'preserved'
    record = {'schema_version': 1, 'case_id': args.case_id, 'old_sha256': sha(old), 'new_sha256': sha(new),
              'behavior_claim': claim}
    if shutil.which('dotnet') is None or not args.tool.exists():
        return {**record, 'status': 'blocked', 'verdict': 'blocked', 'reason': 'dotnet or witness tool unavailable'}
    paths = [str(args.old)] + ([str(args.new)] if args.new else [])
    out = subprocess.run(['dotnet', str(args.tool), *paths], capture_output=True, text=True, timeout=TIMEOUT)
    if out.returncode:
        return {**record, 'status': 'blocked', 'verdict': 'blocked', 'reason': out.stderr[-500:]}
    report = json.loads(out.stdout)
    sides = report['results']
    record['engines'] = [{'name': 'Microsoft.SqlServer.TransactSql.ScriptDom (NuGet)',
                          'version': report['parser_informational'], 'runtime': '.NET ' + report['runtime'],
                          'parsers': ['TSql160Parser(initialQuotedIdentifiers: true)',
                                      'TSql150Parser(initialQuotedIdentifiers: true)']}]
    record['sides'] = []
    for data, side in zip((old, new), sides):
        record['sides'].append({'errors160': len(side['errors160']), 'errors150': len(side['errors150']),
                                'statement_count': len(side['statements']),
                                'statement_starts': _statements_from_offsets(data, [s['offset'] for s in side['statements']]),
                                'statement_types': [s['type'] for s in side['statements']],
                                'generated160_sha256': sha(side['generated160'].encode()) if side['generated160'] else None})
    ok = all(s['errors160'] == 0 for s in record['sides'])
    if args.expect_errors150 is not None:
        record['upstream_expected_errors150'] = args.expect_errors150
        ok = ok and record['sides'][0]['errors150'] == args.expect_errors150
    if args.new:
        record['oracle'] = 'scriptdom_parse_and_regenerate_equal'
        same = record['sides'][0]['generated160_sha256'] == record['sides'][1]['generated160_sha256']
        record['generated_equal'] = same
        ok = ok and same
        record['claim_tested'] = ('both sides parse with 0 TSql160 errors and Sql160ScriptGenerator renders identical '
                                  'scripts from both parse trees (syntactic equivalence under ScriptDom)')
    else:
        record['oracle'] = 'scriptdom_parse_acceptance_and_statement_inventory'
        record['claim_tested'] = ('0 TSql160Parser errors; TSql150Parser error count equals the upstream '
                                  'Only160SyntaxTests expectation; statement inventory from the ScriptDom tree')
    record['status'] = 'executed'; record['verdict'] = 'supports_claim' if ok else 'inconclusive'
    return record


# --- Power Query M: @microsoft/powerquery-parser -------------------------------------------

def powerquery(args):
    data = args.source.read_bytes()
    record = {'schema_version': 1, 'case_id': args.case_id, 'old_sha256': sha(data), 'new_sha256': sha(data),
              'behavior_claim': 'preserved', 'oracle': 'powerquery_parser_lex_parse_and_member_inventory'}
    tool = Path(__file__).resolve().parent / 'witness_tools' / 'powerquery' / 'm_witness.js'
    if shutil.which('node') is None:
        return {**record, 'status': 'blocked', 'verdict': 'blocked', 'reason': 'node unavailable'}
    out = subprocess.run(['node', str(tool), str(args.source)], capture_output=True, text=True, timeout=TIMEOUT,
                         env={**os.environ, 'NODE_PATH': str(args.node_path)})
    if out.returncode:
        return {**record, 'status': 'blocked', 'verdict': 'blocked', 'reason': out.stderr[-500:]}
    report = json.loads(out.stdout); row = report['results'][0]
    record['engines'] = [{'name': report['package'], 'version': report['version'], 'runtime': 'Node.js ' + report['node']}]
    record['parse_ok'] = row['parse_ok']; record['root_kind'] = row.get('root_kind')
    if row.get('members') is not None:
        record['statement_count'] = len(row['members'])
        record['statement_starts'] = _statements_from_offsets(data, [m['offset'] for m in row['members']])
        record['member_names'] = [m['name'] for m in row['members']]
    record['claim_tested'] = ('TaskUtils.tryLexParse(DefaultSettings) reaches an OK parse stage, as the upstream '
                              'lexParseResource test asserts; section members inventoried from the upstream AST')
    record['status'] = 'executed'; record['verdict'] = 'supports_claim' if row['parse_ok'] else 'inconclusive'
    return record


# --- ADF pipeline JSON --------------------------------------------------------------------

def _strict_json(data: bytes):
    def pairs(items):
        keys = [k for k, _ in items]
        if len(keys) != len(set(keys)):
            raise ValueError(f'duplicate object key among {keys}')
        return dict(items)

    def constant(name):
        raise ValueError(f'non-standard JSON constant {name}')
    return json.loads(data.decode('utf-8'), object_pairs_hook=pairs, parse_constant=constant)


ADF_OUTCOMES = ('Succeeded', 'Failed', 'Skipped')
# Microsoft ADF activity dependency semantics (concepts-pipelines-activities): an activity
# runs when every dependsOn entry has one of its dependencyConditions met by that
# predecessor's outcome; Completed means Succeeded or Failed.
ADF_CONDITION = {'Succeeded': {'Succeeded'}, 'Failed': {'Failed'}, 'Skipped': {'Skipped'},
                 'Completed': {'Succeeded', 'Failed'}}


def adf_eligibility(pipeline):
    activities = pipeline.get('properties', {}).get('activities', [])
    matrix = {}
    for activity in activities:
        deps = activity.get('dependsOn') or []
        for dep in deps:
            for outcome in ADF_OUTCOMES:
                allowed = set().union(*(ADF_CONDITION.get(c, set()) for c in dep.get('dependencyConditions', [])))
                matrix[f"{activity['name']} <- {dep['activity']}={outcome}"] = outcome in allowed
    return matrix


def json_pair(args):
    old, new = args.old.read_bytes(), args.new.read_bytes()
    record = {'schema_version': 1, 'case_id': args.case_id, 'old_sha256': sha(old), 'new_sha256': sha(new),
              'behavior_claim': args.claim, 'oracle': 'strict_json_value_equality_and_adf_dependency_matrix',
              'engines': [{'name': 'python json (strict: duplicate keys and NaN/Infinity rejected)',
                           'version': f'Python {".".join(map(str, __import__("sys").version_info[:3]))}'},
                          {'name': 'ADF dependency-condition model (documented semantics; predecessor outcomes '
                                   'Succeeded/Failed/Skipped injected)', 'version': '1'}]}
    try:
        a, b = _strict_json(old), _strict_json(new)
    except ValueError as error:
        return {**record, 'status': 'executed', 'verdict': 'inconclusive', 'reason': str(error)}
    record['values_equal'] = a == b
    ea, eb = adf_eligibility(a), adf_eligibility(b)
    record['eligibility_old'] = ea; record['eligibility_new'] = eb
    record['eligibility_differs'] = ea != eb
    if args.claim == 'preserved':
        ok = a == b
    else:
        ok = a != b
    record['status'] = 'executed'
    record['verdict'] = 'supports_claim' if ok else 'contradicts_claim'
    return record


def from_ledger(args):
    """Run every planned witness of the selected ledger cases; write admission/witnesses/<id>.json."""
    root = args.fixtures.resolve()
    ledger = json.loads((root / 'admission' / 'ledger.json').read_bytes())
    count = 0
    for case in ledger['cases']:
        plan = case.get('witness_plan') or {}
        command = plan.get('command')
        if command not in ('pg-file', 'pair', 'scriptdom', 'powerquery', 'json-pair') \
                or (args.suites and case['suite_id'] not in args.suites):
            continue
        old, new = root / case['old']['path'], root / case['new']['path']
        ns = argparse.Namespace(case_id=case['id'], out=root / 'admission' / 'witnesses' / f"{case['id']}.json")
        if command == 'pg-file':
            ns.source = old; ns.csvlog = args.csvlog
            ns.expected = root / plan['expected'] if plan.get('expected') else None
            record = pg_file(ns)
        elif command == 'pair':
            ns.old, ns.new, ns.claim, ns.order, ns.engines = old, new, plan['claim'], plan['order'], plan['engines']
            record = pair(ns)
        elif command == 'scriptdom':
            ns.old, ns.new, ns.tool = old, (new if new != old else None), args.tool
            ns.expect_errors150 = plan.get('expect_errors150')
            record = scriptdom(ns)
        elif command == 'powerquery':
            ns.source, ns.node_path = old, args.node_path
            record = powerquery(ns)
        else:
            ns.old, ns.new, ns.claim = old, new, plan['claim']
            record = json_pair(ns)
        ns.out.parent.mkdir(parents=True, exist_ok=True)
        ns.out.write_text(json.dumps(record, indent=2, ensure_ascii=False, sort_keys=True) + '\n', encoding='utf-8')
        print(case['id'], record['status'], record['verdict'], flush=True)
        count += 1
    print(f'{count} witnesses written')
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    f = sub.add_parser('pg-file')
    f.add_argument('--case-id', required=True); f.add_argument('--source', type=Path, required=True)
    f.add_argument('--expected', type=Path); f.add_argument('--csvlog', type=Path, required=True)
    f.add_argument('--out', type=Path, required=True)
    p = sub.add_parser('pair')
    p.add_argument('--case-id', required=True); p.add_argument('--old', type=Path, required=True)
    p.add_argument('--new', type=Path, required=True); p.add_argument('--claim', choices=('changed', 'preserved'), required=True)
    p.add_argument('--order', choices=('ordered', 'bag'), required=True)
    p.add_argument('--engines', nargs='+', choices=sorted(ENGINES), default=['postgresql', 'sqlite'])
    p.add_argument('--out', type=Path, required=True)
    t = sub.add_parser('scriptdom')
    t.add_argument('--case-id', required=True); t.add_argument('--old', type=Path, required=True)
    t.add_argument('--new', type=Path); t.add_argument('--expect-errors150', type=int)
    t.add_argument('--tool', type=Path, required=True); t.add_argument('--out', type=Path, required=True)
    m = sub.add_parser('powerquery')
    m.add_argument('--case-id', required=True); m.add_argument('--source', type=Path, required=True)
    m.add_argument('--node-path', type=Path, required=True); m.add_argument('--out', type=Path, required=True)
    j = sub.add_parser('json-pair')
    j.add_argument('--case-id', required=True); j.add_argument('--old', type=Path, required=True)
    j.add_argument('--new', type=Path, required=True)
    j.add_argument('--claim', choices=('changed', 'preserved'), required=True); j.add_argument('--out', type=Path, required=True)
    g = sub.add_parser('ledger', help='run every planned witness recorded in the admission ledger')
    g.add_argument('--fixtures', type=Path, default=Path('tests/fixtures/external'))
    g.add_argument('--suites', nargs='+'); g.add_argument('--csvlog', type=Path)
    g.add_argument('--tool', type=Path, help='built ScriptDomWitness.dll')
    g.add_argument('--node-path', type=Path, help='node_modules holding @microsoft/powerquery-parser@4.0.1')
    args = parser.parse_args(argv)
    if args.command == 'ledger':
        return from_ledger(args)
    record = {'pg-file': pg_file, 'pair': pair, 'scriptdom': scriptdom, 'powerquery': powerquery,
              'json-pair': json_pair}[args.command](args)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(record, indent=2, ensure_ascii=False, sort_keys=True) + '\n', encoding='utf-8')
    print(args.case_id, record['status'], record['verdict'])
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
