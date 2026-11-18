"""#262 SQL and data-platform family for the schema-2 admission ledger (core #268).

Benchmark tooling only. Contributes suites, discovery rows and cases for the registered
IDs sql, plsql, tsql, dax, m, sas, adf, databricks and databricks-workflow. Upstream bytes
are vendored verbatim under ``tests/fixtures/external/upstream/<suite>/`` at the pinned
revision; derived bytes are written under ``admission/cases/<language>/``. Admission is
decided only by the executed witness records under ``admission/witnesses/`` (written by
``scripts/data_family_witness.py``) plus the source review recorded here. IntentumDiff
output is never consulted. Languages with no admissible oracle get no invented cases; their
gaps are documented in docs/EXTERNAL_BENCHMARKS.md and the scorecard README.
"""
from __future__ import annotations

import json
import re

ALL_TYPES = ('ADDITION', 'DELETION', 'MODIFICATION', 'REFACTORING', 'MOVE', 'REORDER')
EDIT_TYPES = ['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE']
CODE_TYPES = ('ADDITION', 'DELETION', 'MODIFICATION', 'MOVE', 'REORDER')

PG, PG_REV = 'postgresql-regress', '7d3e000c5961a544302072058a1184e9a588837b'
SD, SD_REV = 'sqlscriptdom', '3ea69b6fc1ed76b63eca6df868f3d1347b92e9fb'
PQ, PQ_REV = 'powerquery-parser', '84eb5f6f9f8f656dca69e687f514d7a2f86b5ef5'
ADF, ADF_REV = 'azure-datafactory-samples', 'd470e0613b426db4e886cd57ccd6096e016464da'
UT, UT_REV = 'utplsql', '77fc2b49bc288f5fd9523aa98ee76328adeb5769'

PG_LICENCE = 'PostgreSQL'
PG_ATTRIBUTION = ('PostgreSQL regression tests, PostgreSQL Global Development Group (src/test/regress at '
                  'REL_16_15); PostgreSQL Licence, COPYRIGHT retained in upstream/postgresql-regress/COPYRIGHT.')
SD_ATTRIBUTION = 'Microsoft SqlScriptDOM test scripts and baselines, (c) Microsoft Corporation; MIT (LICENSE retained).'
PQ_ATTRIBUTION = ('Microsoft powerquery-parser test resources (copied there from microsoft/DataConnectors), '
                  '(c) Microsoft Corporation; MIT (LICENSE retained).')
ADF_ATTRIBUTION = 'Azure Data Factory sample code, (c) Microsoft Corporation; MIT (LICENSE.txt retained).'
UT_ATTRIBUTION = 'utPLSQL examples, utPLSQL project contributors; Apache-2.0 (LICENSE retained).'

PG_DIALECT = 'PostgreSQL 16 (pg_regress settings: PGTZ=America/Los_Angeles, DateStyle Postgres/MDY, lc_* C)'
PG_PAIR_DIALECT = 'ANSI-style SQL executed on PostgreSQL 16.15 and SQLite 3.53.1'
SD_DIALECT = 'T-SQL, SQL Server 2022 grammar (ScriptDom TSql160Parser, QUOTED_IDENTIFIER on)'
M_DIALECT = 'Power Query M (powerquery-parser 4.0.1 DefaultSettings)'
ADF_DIALECT = 'ADF authoring pipeline JSON (V2), not ARM-wrapped'

WITNESS_NOTE = ('Executed by scripts/data_family_witness.py against the exact case bytes. Engine or parser outcomes '
                'are bounded evidence for the exercised inputs, not program equivalence. IntentumDiff output is not '
                'an input to this oracle.')

PG_STATEMENT_DOMAIN = ('Reference inventory: every statement the PostgreSQL 16.15 server received from psql (its '
                       'own lexer splits the script; csvlog log_statement=all), located in the exact bytes. '
                       'Comparison domain: direct children of the parse-tree root, one per top-level statement, '
                       'each starting at the reference start byte.')


def contribute(ctx):
    ref, write, witness_ref = ctx.ref, ctx.write, ctx.witness_ref
    classification, forbid = ctx.classification, ctx.forbid
    cases = []

    def case(*, case_id, suite, language, dialect, filename, lane, family, old, new, record, upstream_id,
             fields, derivation, claim, assumptions, expected, review, licence, attribution, plan,
             reject=None, basis='executable_witness'):
        witness, verdict_record = witness_ref(case_id)
        verdict = verdict_record['verdict'] if verdict_record else None
        verbatim = derivation[0]['step'] == 'verbatim_upstream'
        admitted = basis == 'executable_witness' and verdict == 'supports_claim' and reject is None
        if admitted:
            cls, status, reason = ('upstream_gold' if verbatim else 'reviewed_external_derived'), 'admitted', \
                review + f' Witness verdict: {verdict}.'
        else:
            cls, status = 'external_seed_only', 'candidate'
            reason = reject or (f'Witness verdict {verdict}; not admitted.' if verdict else 'No executed witness; not admitted.')
        out = {'id': case_id, 'suite_id': suite, 'language_id': language, 'dialect': dialect, 'filename': filename,
               'class': cls, 'lane': lane, 'change_family': family,
               'upstream': {'case_id': upstream_id, 'fields': fields, 'record': record},
               'old': old, 'new': new, 'licence': licence, 'attribution': attribution, 'derivation': derivation,
               'behavior': {'claim': claim, 'assumptions': assumptions},
               'oracle': {'basis': basis if witness or basis != 'executable_witness' else 'candidate_only',
                          'witness': witness, 'byte_identity': old == new,
                          'notes': WITNESS_NOTE if basis == 'executable_witness' else
                          'No reference engine for this dialect is available; no executed oracle.'},
               'admission': {'status': status, 'reason': reason},
               'expected': expected if admitted else None, 'witness_plan': plan}
        cases.append(out)
        return out

    def parse_expected(inventory, evidence):
        exp = {'classification': classification(False, None, True), 'required': [], 'alternatives': [],
               'forbidden': [forbid('change_type', 'byte-identical input has no edit', t) for t in ALL_TYPES]
               + [forbid('fallback', 'reference-accepted source must be parsed, not fall back')],
               'parse': {'side': 'old', 'statement_inventory': None}}
        if inventory is not None:
            exp['parse']['statement_inventory'] = {'count': len(inventory), 'starts': inventory, 'evidence': evidence}
        return exp

    def inventory_of(case_id, key='statement_starts'):
        _, record = witness_ref(case_id)
        if not record or record.get('verdict') != 'supports_claim':
            return None
        if key == 'pg':
            return [s['start_byte'] for s in record['statements']]
        if 'sides' in record:
            return record['sides'][0]['statement_starts']
        return record.get(key)

    # ---------------------------------------------------------------- PostgreSQL regression
    pg_files = {name: ref(f'upstream/{PG}/sql/{name}.sql') for name in
                ('boolean', 'case', 'delete', 'numeric', 'select_having', 'select_implicit', 'update')}
    pg_expected = {name: ref(f'upstream/{PG}/expected/{name}.out') for name in pg_files}
    pg_text = {name: (ctx.ROOT / r['path']).read_text(encoding='utf-8') for name, r in pg_files.items()}
    pg_reject = {
        'numeric': 'pg_regress transcript matches upstream, but psql meta-commands (\\d, COPY ... FROM stdin data '
                   'terminated by \\.) send statements that are not in the file bytes (8 unlocated), so the statement '
                   'inventory is not established; not admitted.',
        'update': 'pg_regress transcript matches upstream, but the file contains an intentional syntax error '
                  '(SQLSTATE 42601) and \\set variables whose expansion sends 53 statements not present in the file '
                  'bytes; parse acceptance and inventory are not established; not admitted.',
    }
    for name, source in pg_files.items():
        derived = name == 'boolean'
        if derived:  # strip the single psql meta-command line: the SQL payload, not the psql host script
            text = pg_text[name]
            meta = [line for line in text.splitlines(keepends=True) if line.startswith('\\')]
            if meta != ["\\pset null '(null)'\n"]:
                raise ValueError('boolean.sql meta-command precondition failed')
            source = write(f'admission/cases/sql/pgreg/boolean-payload.sql', text.replace(meta[0], '', 1))
        case_id = f'pgreg-{name}-statements'
        inventory = inventory_of(case_id, 'pg')
        case(case_id=case_id, suite=PG, language='sql', dialect=PG_DIALECT, filename='fixture.sql',
             lane='parser_fidelity', family='statement_inventory', old=source, new=source, record=pg_expected[name],
             upstream_id=f'src/test/regress/sql/{name}.sql', fields=['sql', 'expected output'],
             derivation=[{'step': 'derived_transformation', 'description': "psql meta-command line \\pset null "
                          "'(null)' removed to obtain the SQL payload of the psql script; all SQL bytes unchanged",
                          'tool': 'scripts/external_families/family_262_sql_data.py'}] if derived else
             [{'step': 'verbatim_upstream', 'description': f'src/test/regress/sql/{name}.sql at REL_16_15, unmodified'}],
             claim='preserved', assumptions='Identity control in the parser-fidelity lane: the reference PostgreSQL '
             'parser accepts every statement; the upstream pg_regress transcript reproduces expected/' + name + '.out '
             'on PostgreSQL 16.15 (verbatim file).',
             expected=parse_expected(inventory, PG_STATEMENT_DOMAIN),
             review='Reference transcript reproduced byte-for-byte; every server-received statement located in the '
                    'exact bytes; no SQLSTATE 42601.', licence=PG_LICENCE, attribution=PG_ATTRIBUTION,
             plan={'command': 'pg-file', 'expected': None if derived else pg_expected[name]['path'],
                   'verbatim_transcript_check': pg_expected[name]['path']},
             reject=pg_reject.get(name))

    # Change-understanding pairs: upstream fixture setup + one upstream query (old) or its
    # reviewed transformation (new). Same engines, same data; result sets compared.
    def setup_of(name, last):
        text = pg_text[name]; end = text.index(last) + len(last)
        return text[:end]

    def statement(name, anchor):
        text = pg_text[name]
        if text.count(anchor) != 1:
            raise ValueError(f'{name}: anchor not unique: {anchor!r}')
        start = text.index(anchor); end = text.index(';\n', start) + 2
        return text[start:end]

    def edit(query, old, new):
        if query.count(old) != 1:
            raise ValueError(f'edit precondition failed: {old!r}')
        return query.replace(old, new)

    sh_setup = setup_of('select_having', "INSERT INTO test_having VALUES (9, 4, 'CCCC', 'j');\n")
    si_setup = setup_of('select_implicit', "INSERT INTO test_missing_target VALUES (9, 4, 'CCCC', 'j');\n")
    case_setup = setup_of('case', 'INSERT INTO CASE2_TBL VALUES (NULL, -6);\n')
    q1 = statement('select_having', 'SELECT b, c FROM test_having\n\tGROUP BY b, c HAVING count(*) = 1')
    q2 = statement('select_having', 'SELECT b, c FROM test_having\n\tGROUP BY b, c HAVING b = 3')
    q4 = statement('select_having', 'SELECT c, max(a) FROM test_having')
    qx = statement('select_implicit', 'SELECT x.b, count(*) FROM test_missing_target x')
    qc = statement('case', 'SELECT *\n  FROM CASE_TBL a, CASE2_TBL b\n  WHERE COALESCE(a.f, b.i, b.j) = 2;')
    qm = statement('case', 'SELECT\n  CASE WHEN i >= 3 THEN (i + i)')

    preserved_style = lambda: {'classification': classification(False, True, None), 'required': [], 'alternatives': [],
                               'forbidden': [forbid('change_type', 'presentation-only edit is not a code edit', t)
                                             for t in CODE_TYPES]
                               + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
    preserved_comment = lambda: {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
                                 'forbidden': [forbid('change_type', 'a comment is not a code edit', t)
                                               for t in CODE_TYPES]
                                 + [forbid('fallback', 'valid source must be reviewed, not fall back')]}

    def changed(old_b, new_b, what):
        old_r, new_r = ctx.minimal_region(old_b, new_b)
        return {'classification': classification(True, False, False), 'alternatives': [],
                'required': [{'kind': 'edit_region', 'change_types_any_of': EDIT_TYPES, 'old_range': old_r,
                              'new_range': new_r, 'evidence': f'{what}: a reported edit must touch bytes '
                              f'{old_r["start_byte"]}..{old_r["end_byte"]} (old) / {new_r["start_byte"]}..'
                              f'{new_r["end_byte"]} (new)'}],
                'forbidden': [forbid('style_only', 'a witnessed result-set change is not style-only'),
                              forbid('no_changes', 'the edit must be reported'),
                              forbid('fallback', 'valid source must be reviewed, not fall back')]}

    def rewrite(old_b, new_b, what, refactor=False):
        old_r, new_r = ctx.minimal_region(old_b, new_b)
        types = ['REFACTORING', 'MODIFICATION'] if refactor else list(ALL_TYPES)
        exp = {'classification': classification(None, False, False), 'alternatives': [],
               'required': [{'kind': 'edit_region', 'change_types_any_of': types, 'old_range': old_r,
                             'new_range': new_r, 'evidence': f'{what} must be explained structurally'}],
               'forbidden': [forbid('style_only', 'an equivalent rewrite is not formatting')]}
        if refactor:
            exp['forbidden'] += [forbid('change_type', 'an alias rename does not add or delete an entity', t)
                                 for t in ('ADDITION', 'DELETION')]
        return exp

    pairs = [
        # id, source file, setup, old query, new query, claim, family, order, transformation, expected kind
        ('keyword-case', 'select_having', sh_setup, q1,
         'select b, c from test_having\n\tgroup by b, c having count(*) = 1 order by b, c;\n',
         'preserved', 'keyword_case', 'ordered', 'SQL keywords lower-cased; identifiers and literals unchanged '
         '(keywords are case-insensitive in SQL)', 'style'),
        ('whitespace', 'select_having', sh_setup, q1,
         'SELECT   b,c\nFROM test_having GROUP BY b,c\n    HAVING count(*)=1\nORDER BY b,c;\n',
         'preserved', 'whitespace', 'ordered', 'whitespace between tokens changed only', 'style'),
        ('comment', 'select_having', sh_setup, q2,
         '-- groups with b = 3 only\nSELECT b, c FROM test_having /* grouped */\n\tGROUP BY b, c HAVING b = 3 ORDER BY b, c;\n',
         'preserved', 'comment', 'ordered', 'a line comment and a block comment added', 'comment'),
        ('having-to-where', 'select_having', sh_setup, q2,
         'SELECT b, c FROM test_having\n\tWHERE b = 3 GROUP BY b, c ORDER BY b, c;\n',
         'preserved', 'equivalent_rewrite', 'ordered', 'HAVING b = 3 moved to WHERE; b is a grouping column, so '
         'filtering groups or rows is equivalent (upstream comment: "HAVING is effectively equivalent to WHERE in '
         'this case")', 'rewrite'),
        ('predicate', 'select_having', sh_setup, q1, edit(q1, 'HAVING count(*) = 1', 'HAVING count(*) >= 1'),
         'changed', 'predicate', 'ordered', 'HAVING comparison operator = -> >=', 'changed'),
        ('literal', 'select_having', sh_setup, q2, edit(q2, 'HAVING b = 3', 'HAVING b = 4'),
         'changed', 'literal', 'ordered', 'predicate literal 3 -> 4', 'changed'),
        ('ordering', 'select_having', sh_setup, q4, edit(q4, 'ORDER BY c;', 'ORDER BY c DESC;'),
         'changed', 'ordering', 'ordered', 'ORDER BY c -> ORDER BY c DESC (c is the unique group key, a total order)',
         'changed'),
        ('alias-rename', 'select_implicit', si_setup, qx,
         re.sub(r'\by\b', 'q', re.sub(r'\bx\b', 'p', qx)),
         'preserved', 'rename_alias', 'ordered', 'table aliases x -> p and y -> q renamed at every use; result '
         'column names are unchanged (b, count)', 'refactor'),
        ('join-predicate', 'select_implicit', si_setup, qx, edit(qx, 'WHERE x.a = y.a', 'WHERE x.a = y.b'),
         'changed', 'join', 'ordered', 'self-join predicate x.a = y.a -> x.a = y.b', 'changed'),
        ('inner-join', 'case', case_setup, qc,
         'SELECT *\n  FROM CASE_TBL a INNER JOIN CASE2_TBL b\n  ON COALESCE(a.f, b.i, b.j) = 2;\n',
         'preserved', 'equivalent_rewrite', 'bag', 'comma join + WHERE rewritten as INNER JOIN ... ON with the same '
         'predicate (equivalent by the SQL definition of inner join)', 'rewrite'),
        ('left-join', 'case', case_setup, qc,
         'SELECT *\n  FROM CASE_TBL a LEFT JOIN CASE2_TBL b\n  ON COALESCE(a.f, b.i, b.j) = 2;\n',
         'changed', 'join', 'bag', 'comma join + WHERE rewritten as LEFT JOIN ... ON: unmatched CASE_TBL rows are '
         'now preserved with NULLs', 'changed'),
        ('case-boundary', 'case', case_setup, qm, edit(qm, 'WHEN i >= 3', 'WHEN i >= 2'),
         'changed', 'predicate', 'bag', 'CASE condition boundary i >= 3 -> i >= 2', 'changed'),
    ]
    for pid, name, setup, old_q, new_q, claim, family, order, description, kind in pairs:
        case_id = f'pgreg-{name.replace("_", "-")}-{pid}'
        old_text, new_text = setup + '\n' + old_q, setup + '\n' + new_q
        old = write(f'admission/cases/sql/pgreg/{pid}-old.sql', old_text)
        new = write(f'admission/cases/sql/pgreg/{pid}-new.sql', new_text)
        ob, nb = old_text.encode(), new_text.encode()
        expected = {'style': preserved_style, 'comment': preserved_comment}.get(kind, lambda: None)() or \
            (changed(ob, nb, description) if kind == 'changed' else rewrite(ob, nb, description, kind == 'refactor'))
        case(case_id=case_id, suite=PG, language='sql', dialect=PG_PAIR_DIALECT, filename='fixture.sql',
             lane='change_understanding', family=family, old=old, new=new, record=pg_files[name],
             upstream_id=f'src/test/regress/sql/{name}.sql', fields=['fixture setup statements', 'query statement'],
             derivation=[{'step': 'derived_transformation', 'description': 'upstream setup (CREATE/INSERT, verbatim) '
                          f'+ one upstream query (verbatim, old) / reviewed transformation (new): {description}',
                          'tool': 'scripts/external_families/family_262_sql_data.py'}],
             claim=claim, assumptions=f'Final-statement result set on the upstream fixture data, compared '
             f'{"in order" if order == "ordered" else "as a bag"} within each engine (PostgreSQL 16.15 and SQLite '
             '3.53.1). ' + ('Preservation rests on the stated SQL equivalence; the engines confirm it on this data.'
                            if claim == 'preserved' else 'The engines observe different results on this data.'),
             expected=expected, review=f'Exact source delta reviewed: {description}.', licence=PG_LICENCE,
             attribution=PG_ATTRIBUTION, plan={'command': 'pair', 'claim': claim, 'order': order,
                                               'engines': ['postgresql', 'sqlite']})

    # ---------------------------------------------------------------- Microsoft SqlScriptDOM
    errors150 = {'MergeStatementTests160': 5, 'SelectStatementTests160': 5, 'ShiftOperatorTests160': 3,
                 'StringConcatOperatorTests160': 11, 'WindowClauseTests160': 15}
    sd_config = ref(f'upstream/{SD}/Test/SqlDom/Only160SyntaxTests.cs')
    config_text = (ctx.ROOT / sd_config['path']).read_text(encoding='utf-8')
    for name, n150 in errors150.items():
        if f'new ParserTest160("{name}.sql",' not in config_text or f'nErrors150: {n150})' not in \
                config_text.split(f'"{name}.sql"')[1].split('\n')[0]:
            raise ValueError(f'{name}: upstream nErrors150 precondition failed')
        script = ref(f'upstream/{SD}/Test/SqlDom/TestScripts/{name}.sql')
        baseline = ref(f'upstream/{SD}/Test/SqlDom/Baselines160/{name}.sql')
        case_id = f'scriptdom-{name}-statements'
        case(case_id=case_id, suite=SD, language='tsql', dialect=SD_DIALECT, filename='fixture.tsql',
             lane='parser_fidelity', family='statement_inventory', old=script, new=script, record=sd_config,
             upstream_id=f'Only160SyntaxTests: ParserTest160("{name}.sql")', fields=['TestScripts', 'nErrors150'],
             derivation=[{'step': 'verbatim_upstream', 'description': f'Test/SqlDom/TestScripts/{name}.sql, unmodified '
                          '(UTF-8 BOM retained)'}],
             claim='preserved', assumptions='Identity control in the parser-fidelity lane: Microsoft ScriptDom '
             'TSql160Parser accepts the script with 0 errors and TSql150Parser reproduces the upstream nErrors150.',
             expected=parse_expected(inventory_of(case_id),
                                     'Reference inventory: every statement of every batch in the ScriptDom TSqlScript '
                                     'tree (StartOffset mapped to file bytes). Comparison domain: direct children of '
                                     'the parse-tree root, each starting at the reference start byte.'),
             review='ScriptDom parse executed; upstream error-count expectation reproduced.', licence='MIT',
             attribution=SD_ATTRIBUTION, plan={'command': 'scriptdom', 'expect_errors150': n150})
        case_id = f'scriptdom-{name}-baseline'
        case(case_id=case_id, suite=SD, language='tsql', dialect=SD_DIALECT, filename='fixture.tsql',
             lane='change_understanding', family='generator_roundtrip', old=script, new=baseline, record=sd_config,
             upstream_id=f'Only160SyntaxTests: ParserTest160("{name}.sql") script vs Baselines160 baseline',
             fields=['TestScripts', 'Baselines160'],
             derivation=[{'step': 'verbatim_upstream', 'description': f'old = TestScripts/{name}.sql, new = '
                          f'Baselines160/{name}.sql: the upstream pair whose baseline is ScriptDom\'s regenerated script'}],
             claim='preserved', assumptions='Syntactic equivalence under ScriptDom: both sides parse with 0 TSql160 '
             'errors and Sql160ScriptGenerator renders the same script from both trees. Statement terminators, '
             'whitespace, keyword case and GO placement differ; no SQL Server execution is claimed.',
             expected={'classification': classification(False, None, None), 'required': [], 'alternatives': [],
                       'forbidden': [forbid('fallback', 'valid source must be reviewed, not fall back')]},
             review='Upstream pair; ScriptDom regeneration equality executed.', licence='MIT',
             attribution=SD_ATTRIBUTION, plan={'command': 'scriptdom'})

    # ---------------------------------------------------------------- Power Query M
    pq_test = ref(f'upstream/{PQ}/src/test/resourceTest/lexParseResource.test.ts')
    base = f'upstream/{PQ}/src/test/resources/microsoft-DataConnectors'
    for rel in ('HelloWorld/HelloWorld.pq', 'HelloWorldWithDocs/HelloWorldWithDocs.pq', 'NavigationTable/NavigationTable.pq',
                'OAuthPKCE/PKCESample.pq', 'TripPin/1-OData/TripPin.pq', 'TripPin/2-Rest/TripPin.pq',
                'TripPin/3-NavTables/TripPin.pq', 'HelloWorld/HelloWorld.query.pq', 'TripPin/1-OData/TripPin.query.pq'):
        source = ref(f'{base}/{rel}')
        slug = re.sub(r'[^A-Za-z0-9]+', '-', rel.removesuffix('.pq')).strip('-').lower()
        case_id = f'pqparser-{slug}-parse'
        inventory = inventory_of(case_id)
        case(case_id=case_id, suite=PQ, language='m', dialect=M_DIALECT, filename='fixture.m',
             lane='parser_fidelity', family='section_members' if inventory else 'parse_acceptance',
             old=source, new=source, record=pq_test, upstream_id=f'lexParseResource: {rel}', fields=['resource file'],
             derivation=[{'step': 'verbatim_upstream', 'description': f'src/test/resources/microsoft-DataConnectors/{rel}, unmodified'}],
             claim='preserved', assumptions='Identity control in the parser-fidelity lane: Microsoft powerquery-parser '
             'lexes and parses the resource with an OK parse stage, as the upstream resource test asserts.',
             expected=parse_expected(inventory, 'Reference inventory: section members of the powerquery-parser AST '
                                     '(first-token code-unit offset mapped to file bytes). Comparison domain: direct '
                                     'children of the parse-tree root, one per section member.'),
             review='powerquery-parser lex/parse executed on the exact bytes.', licence='MIT',
             attribution=PQ_ATTRIBUTION, plan={'command': 'powerquery'})

    # ---------------------------------------------------------------- Azure Data Factory
    adf_dir = f'upstream/{ADF}/V1-V2Migration/sampledatasets/V2/Pipeline'
    first, second = ref(f'{adf_dir}/adf_First_Pipeline.json'), ref(f'{adf_dir}/adf_Second_Pipeline.json')
    for name, source in (('first', first), ('second', second)):
        case(case_id=f'adf-samples-{name}-parse', suite=ADF, language='adf', dialect=ADF_DIALECT, filename='fixture.json',
             lane='parser_fidelity', family='parse_acceptance', old=source, new=source, record=source,
             upstream_id=source['path'].removeprefix(f'upstream/{ADF}/'), fields=['sample file'],
             derivation=[{'step': 'verbatim_upstream', 'description': 'vendor sample pipeline, unmodified'}],
             claim='preserved', assumptions='Identity control: strict JSON (no duplicate keys, no NaN) accepted. '
             'No ADF service or schema validation is claimed (schema host blocked).',
             expected=parse_expected(None, ''), review='Strict JSON parse executed.', licence='MIT',
             attribution=ADF_ATTRIBUTION, plan={'command': 'json-pair', 'claim': 'preserved'})
    text = (ctx.ROOT / first['path']).read_text(encoding='utf-8')
    obj = json.loads(text)
    activity = obj['properties']['activities'][1]
    reordered = dict(obj); reordered['properties'] = {'activities': [obj['properties']['activities'][0],
                                                                    {k: activity[k] for k in reversed(list(activity))}]}
    adf_pairs = [
        ('key-reorder', json.dumps(reordered, indent=2), 'preserved', 'key_order',
         'member order of the FileSystemtoBlob activity object reversed; arrays and values unchanged'),
        ('minified', json.dumps(obj, separators=(',', ':')), 'preserved', 'whitespace',
         'whitespace between JSON tokens removed (same member order, values and arrays)'),
        ('dependency-condition', edit(text, '"Succeeded"', '"Completed"'), 'changed', 'dependency_condition',
         'FileSystemtoBlob dependency condition Succeeded -> Completed: it now also runs after a failed validation'),
        ('policy-timeout', edit(text, '"timeout": "01:00:00"', '"timeout": "02:00:00"'), 'changed', 'literal',
         'copy activity policy timeout 01:00:00 -> 02:00:00'),
    ]
    if json.dumps(obj, indent=2) != text:
        raise ValueError('adf_First_Pipeline.json is not in json.dumps(indent=2) form; re-derive the controls')
    for pid, new_text, claim, family, description in adf_pairs:
        new = write(f'admission/cases/adf/first-{pid}-new.json', new_text)
        ob, nb = text.encode(), new_text.encode()
        if claim == 'changed':
            expected = changed(ob, nb, description)
        elif family == 'whitespace':
            expected = preserved_style()
        else:
            expected = {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
                        'forbidden': [forbid('change_type', 'mapping members are compared by key; reordering them is '
                                             'not a value edit', t) for t in ('ADDITION', 'DELETION', 'MODIFICATION')]
                        + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
        case(case_id=f'adf-samples-first-{pid}', suite=ADF, language='adf', dialect=ADF_DIALECT, filename='fixture.json',
             lane='change_understanding', family=family, old=first, new=new, record=first,
             upstream_id=first['path'].removeprefix(f'upstream/{ADF}/'), fields=['sample file'],
             derivation=[{'step': 'derived_transformation', 'description': description,
                          'tool': 'scripts/external_families/family_262_sql_data.py'}],
             claim=claim, assumptions='Behaviour = the pipeline definition submitted to ADF. Mappings compare by key, '
             'arrays by position; dependency eligibility per Microsoft\'s documented dependency conditions, evaluated '
             'with injected predecessor outcomes (no ADF service run).',
             expected=expected, review=f'Exact source delta reviewed: {description}.', licence='MIT',
             attribution=ADF_ATTRIBUTION, plan={'command': 'json-pair', 'claim': claim})

    # ---------------------------------------------------------------- utPLSQL (seed only)
    for rel in ('examples/between_string/betwnstr.sql', 'examples/between_string/test_betwnstr.pks',
                'examples/between_string/test_betwnstr.pkb', 'examples/award_bonus/award_bonus.sql'):
        source = ref(f'upstream/{UT}/{rel}')
        case(case_id='utplsql-' + re.sub(r'[^a-z0-9]+', '-', rel.lower().removeprefix('examples/')).strip('-'),
             suite=UT, language='plsql', dialect='Oracle PL/SQL', filename='fixture.sql', lane='parser_fidelity',
             family='parse_acceptance', old=source, new=source, record=source, upstream_id=rel, fields=['example source'],
             derivation=[{'step': 'verbatim_upstream', 'description': f'{rel}, unmodified'}],
             claim='unknown', assumptions='No Oracle Database is available, so no compile/run oracle was executed.',
             expected=None, review='', licence='Apache-2.0', attribution=UT_ATTRIBUTION, plan=None, basis='candidate_only',
             reject='BLOCKED: no Oracle Database engine in this environment; utPLSQL assertions need Oracle. Seed only.')

    suites = [
        _suite(ctx, PG, 'PostgreSQL regression tests (src/test/regress SQL + expected output)', 'REL_16_15',
               'https://github.com/postgres/postgres/tree/REL_16_15/src/test/regress', PG_REV, PG_LICENCE,
               'COPYRIGHT at REL_16_15 (PostgreSQL Licence), retained', PG_ATTRIBUTION,
               'official upstream regression suite', PG, 'postgres/postgres'),
        _suite(ctx, SD, 'Microsoft SqlScriptDOM test scripts and Baselines160', 'main @ ' + SD_REV[:12],
               'https://github.com/microsoft/SqlScriptDOM', SD_REV, 'MIT', 'LICENSE at the pinned commit, retained',
               SD_ATTRIBUTION, 'official upstream regression suite', SD, 'microsoft/SqlScriptDOM',
               extra=['Witness parser: NuGet Microsoft.SqlServer.TransactSql.ScriptDom 180.140.0 (informational '
                      'version 18.0.140.0+9286938f), the newest release whose release notes exist at the pinned '
                      'commit; it is not a build of the pinned commit.']),
        _suite(ctx, PQ, 'Microsoft powerquery-parser lex/parse resource suite', '4.0.1 (package.json) @ ' + PQ_REV[:12],
               'https://github.com/microsoft/powerquery-parser', PQ_REV, 'MIT', 'LICENSE at the pinned commit, retained',
               PQ_ATTRIBUTION, 'official upstream regression suite', PQ, 'microsoft/powerquery-parser',
               extra=['Witness parser: npm @microsoft/powerquery-parser@4.0.1 (integrity sha512-hYibLzi9M01RSQ8M'
                      'lthEJaX2lhIaWjNHrcUd1HMcTqs487XatdmgvvsJ6ql4P7bRFDjQ6z2GN3FAMJ2UL3WOow==), matching the '
                      'package.json version at the pinned commit; not a build of the commit.']),
        _suite(ctx, ADF, 'Microsoft Azure Data Factory samples', 'main @ ' + ADF_REV[:12],
               'https://github.com/Azure/Azure-DataFactory', ADF_REV, 'MIT', 'LICENSE.txt at the pinned commit, retained',
               ADF_ATTRIBUTION, 'qualified external candidate (vendor samples)', ADF, 'Azure/Azure-DataFactory',
               extra=['BLOCKED: schema.management.azure.com (ADF JSON schema) refused by the egress proxy; no schema '
                      'validation is claimed and no other schema source was substituted.']),
        _suite(ctx, UT, 'utPLSQL examples', 'develop @ ' + UT_REV[:12], 'https://github.com/utPLSQL/utPLSQL', UT_REV,
               'Apache-2.0', 'LICENSE at the pinned commit, retained', UT_ATTRIBUTION,
               'established community / compatibility suite', UT, 'utPLSQL/utPLSQL',
               extra=['BLOCKED: no Oracle Database available; examples are retained as seed-only candidates.']),
    ]
    discovery = [
        _disc(PG, 'sql', 'parser_fidelity', 222, 5, 'standalone regress file (no test_setup dependency) whose '
              'pg_regress transcript reproduces expected/<name>.out on PostgreSQL 16.15, with no SQLSTATE 42601 and '
              'every server-received statement located in the bytes. 20 of 222 files were evaluated (aggregates, '
              'boolean, case, delete, insert, int4, join, limit, numeric, select_distinct, select_having, '
              'select_implicit, strings, subselect, text, union, update, varchar, window, with); 7 reproduce the '
              'transcript standalone and 5 meet the rule: 4 verbatim (upstream_gold) and boolean, admitted as its '
              'SQL payload without the one psql meta-command line (reviewed_external_derived row).', ['boolean', 'case', 'delete', 'select_having', 'select_implicit']),
        _disc(PG, 'sql', 'change_understanding', 0, 0, 'PostgreSQL regression provides single-source transcripts, no '
              'before/after pairs; all change cases are reviewed derivations', []),
        _disc(SD, 'tsql', 'parser_fidelity', 492, 41, 'TestScripts/*.sql with a Baselines160 counterpart (TSql160 '
              'expectation: 0 errors)', sorted(errors150)),
        _disc(SD, 'tsql', 'change_understanding', 41, 41, 'TestScripts/X.sql with Baselines160/X.sql (upstream '
              'script/regenerated-baseline pair)', sorted(errors150)),
        _disc(PQ, 'm', 'parser_fidelity', 78, 78, '.pq/.pqm resource exercised by lexParseResource.test.ts',
              ['HelloWorld', 'HelloWorldWithDocs', 'NavigationTable', 'PKCESample', 'TripPin 1-3', 'HelloWorld.query',
               'TripPin 1 query']),
        _disc(ADF, 'adf', 'parser_fidelity', 2, 2, 'V2 authoring-format pipeline files in V1-V2Migration/'
              'sampledatasets/V2/Pipeline (trigger files excluded); ARM-wrapped templates are a different format',
              ['adf_First_Pipeline.json', 'adf_Second_Pipeline.json']),
        _disc(ADF, 'adf', 'change_understanding', 0, 0, 'vendor samples provide no before/after pairs; all change '
              'cases are reviewed derivations', []),
        _disc(UT, 'plsql', 'parser_fidelity', 489, 0, 'needs an Oracle Database compile/run oracle (unavailable)', []),
    ]
    return {'suites': suites, 'discovery': discovery, 'cases': cases}


def _suite(ctx, suite_id, name, version, url, revision, licence, licence_evidence, attribution, evidence_class,
           directory, repo, extra=()):
    files = sorted((p for p in (ctx.ROOT / 'upstream' / directory).rglob('*') if p.is_file()), key=lambda p: str(p))
    return {'suite_id': suite_id, 'name': name, 'version': version, 'url': url, 'revision': revision,
            'licence': licence, 'licence_evidence': licence_evidence, 'attribution': attribution,
            'evidence_class': evidence_class,
            'acquisition': {
                'method': f'git clone --filter=blob:none of https://github.com/{repo} at {revision}; selected blobs '
                          'written verbatim (2026-11-17)',
                'files': [ctx.ref(str(p.relative_to(ctx.ROOT))) for p in files],
                'verification': [f'git ls-remote https://github.com/{repo} resolved the pinned revision {revision}.',
                                 'Every retained file SHA-256 equals the bytes served by raw.githubusercontent.com/'
                                 f'{repo}/{revision}/<path> (2026-11-17).', *extra]}}


def _disc(suite, language, lane, discovered, eligible, rule, imported):
    return {'suite_id': suite, 'language_id': language, 'lane': lane, 'discovered': discovered, 'eligible': eligible,
            'eligibility_rule': rule, 'imported_rows': imported,
            'holdout': 'rows not imported remain unadmitted; no task-level holdout split is claimed'}
