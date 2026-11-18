#!/usr/bin/env python3
"""Extract and witness the #267 build/configuration/infrastructure cases (core #268 ledger).

Benchmark tooling only. Every oracle is a reference tool run outside IntentumDiff:

- dockerfile: BuildKit v0.17.3 ``parser.Parse`` + ``instructions.Parse`` (``scripts/witness_tools/buildinfra``)
- hcl: hashicorp/hcl/v2 v2.23.0 ``hclsyntax.ParseConfig`` / ``hclwrite.Format`` (same tool)
- gomod: golang.org/x/mod v0.22.0 ``modfile.Parse`` (same tool) and cmd/go ``go mod edit -json``
- gitignore: ``git check-ignore`` / ``git ls-files -X`` reproducing t0008 expectations
- cmake: the upstream RunCMake harness (``Tests/RunCMake/RunCMake.cmake``) and plain configure runs
- make: the upstream ``run_make_tests.pl`` harness and plain ``make`` runs
- puppet: the puppet gem (``scripts/witness_tools/puppet/oracle.rb``)

    python scripts/build_infra_witness.py extract   # make harness makefiles + puppet spec manifests
    python scripts/build_infra_witness.py witness   # run every oracle, write admission/witnesses/build-infra/

Tools are found on PATH or through BUILDINFRA_TOOL (the built Go witness tool), PUPPET_RUBY (a ruby that
can ``require 'puppet'``, e.g. with GEM_HOME set), GO, GIT, CMAKE, MAKE and PERL. A missing tool gives a
``blocked`` record, never an admission. After ``witness``, rebuild the ledger with
``scripts/build_external_admission.py``.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent / 'tests' / 'fixtures' / 'external'
_spec = importlib.util.spec_from_file_location('family_267', HERE / 'external_families' / 'family_267_build_infra.py')
family = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(family)
UP = ROOT / family.UP
WIT = ROOT / family.WITNESSES

TOOL = os.environ.get('BUILDINFRA_TOOL', 'buildinfra')
RUBY = os.environ.get('PUPPET_RUBY', 'ruby')
GO = os.environ.get('GO', 'go'); GIT = os.environ.get('GIT', 'git'); CMAKE = os.environ.get('CMAKE', 'cmake')
MAKE = os.environ.get('MAKE', 'make'); PERL = os.environ.get('PERL', 'perl')
PUPPET_ORACLE = HERE / 'witness_tools' / 'puppet' / 'oracle.rb'
PUPPET_EXTRACT = HERE / 'witness_tools' / 'puppet' / 'extract_specs.rb'
# Errors GNU Make raises while reading a makefile. `unterminated variable reference` is excluded: make raises it
# when the variable is expanded, not when the line is read (`x = $(foo` alone runs cleanly on make 4.3).
MAKE_READ_ERRORS = re.compile(r'\*\*\* (missing separator|missing .endif.|'
                              r'invalid syntax in conditional|extraneous .(endef|endif|else).|missing .endef.|'
                              r'recipe commences before first target|commands commence before first target|'
                              r'empty variable name|missing target pattern|multiple target patterns|'
                              r'mixed implicit and normal rules|only one .else. per conditional|'
                              r'invalid .override. directive)')


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run(cmd, **kw):
    kw.setdefault('capture_output', True)
    kw.setdefault('timeout', 120)
    return subprocess.run(cmd, **kw)


def first_line(cmd):
    try:
        out = run(cmd)
        return (out.stdout or out.stderr).decode(errors='replace').splitlines()[0].strip()
    except (OSError, IndexError, subprocess.TimeoutExpired):
        return None


def engines():
    mods = (HERE / 'witness_tools' / 'buildinfra' / 'go.mod').read_text()
    pins = dict(re.findall(r'^\t(github\.com/moby/buildkit|github\.com/hashicorp/hcl/v2|golang\.org/x/mod) (v\S+)', mods, re.M))
    return {'buildinfra': {'go_module_pins': pins, 'go': first_line([GO, 'version'])},
            'go': first_line([GO, 'version']), 'git': first_line([GIT, '--version']),
            'cmake': first_line([CMAKE, '--version']), 'make': first_line([MAKE, '--version']),
            'perl': first_line([PERL, '-e', 'print "perl $^V\\n"']),
            'puppet': first_line([RUBY, '-e', "require 'puppet'; puts \"puppet #{Puppet.version} ruby #{RUBY_VERSION}\""])}


ENGINES = {}


def write_record(case, verdict, review, oracle, used, **observations):
    record = {'schema_version': 1, 'case_id': case['id'], 'old_sha256': case['old']['sha256'],
              'new_sha256': case['new']['sha256'], 'behavior_claim': case['claim'], 'oracle': oracle,
              'engines': [{'name': name, 'version': ENGINES.get(name)} for name in used],
              'verdict': verdict, 'review': review, **observations}
    path = WIT / f"{case['id']}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(record, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
    return verdict


def tool_json(mode, path):
    out = run([TOOL, mode, str(path)])
    if out.returncode:
        raise RuntimeError(f'{mode} tool failed on {path}: {out.stderr.decode(errors="replace")[:400]}')
    return json.loads(out.stdout)


def data(ref):
    return (ROOT / ref['path']).read_bytes()


# ------------------------------------------------------------------ extraction

def extract_make():
    scripts = ['variables/flavors', 'variables/define', 'variables/automatic', 'variables/negative',
                                'variables/special', 'features/order_only', 'features/conditionals',
                                'features/targetvars', 'features/comments', 'features/escape', 'functions/foreach']
    out_dir = ROOT / 'admission/cases/make/harness'
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)
    documents = []
    with tempfile.TemporaryDirectory(prefix='make-harness-') as tmp:
        tests = Path(tmp) / 'tests'
        shutil.copytree(UP / 'gnu-make-tests' / 'tests', tests)
        (tests / 'config-flags.pm').write_text(re.sub(r'@[A-Z_]+@', '', (tests / 'config-flags.pm.in').read_text()))
        driver = (tests / 'run_make_tests.pl').read_text()
        anchor1 = '  local ($makestring, $options, $answer, $err_code, $timeout) = @_;\n'
        anchor2 = '  &compare_output($answer, &get_logfile(1));\n'
        if driver.count(anchor1) != 1 or driver.count(anchor2) != 1:
            raise RuntimeError('run_make_tests.pl patch anchors not found')
        driver = driver.replace(anchor1, anchor1 + '  my $__raw = $makestring;\n')
        driver = driver.replace(anchor2, (
            '  my $__ok = &compare_output($answer, &get_logfile(1));\n'
            '  { use JSON::PP; local $/; my $__mk = "";\n'
            '    if (open(my $__f, "<", $makefile)) { binmode $__f; $__mk = <$__f>; close $__f; }\n'
            '    open(my $__log, ">>", "@@LOG@@") or die;\n'
            '    print $__log JSON::PP->new->canonical->encode({line => $call[2], file => $call[1],\n'
            '      raw => $__raw, makefile => $__mk, options => (ref($options) ? join(" ", @$options) : $options),\n'
            '      answer => $answer, err_code => $err_code, passed => $__ok}), "\\n"; close $__log; }\n'))
        for script in scripts:
            # The harness resets the environment for every test, so the log path is written into the patch.
            log = Path(tmp) / f'{script.replace("/", "_")}.jsonl'
            (tests / 'run_make_tests.pl').write_text(driver.replace('@@LOG@@', str(log)))
            env = dict(os.environ, LC_ALL='C', LANG='C')
            res = run([PERL, 'run_make_tests.pl', '-make', shutil.which(MAKE) or MAKE, script], cwd=tests, env=env,
                      timeout=600)
            source = (UP / 'gnu-make-tests' / 'tests' / 'scripts' / script).read_text(encoding='utf-8')
            index = 0
            for line in log.read_text().splitlines() if log.exists() else []:
                rec = json.loads(line)
                if rec['raw'] is None:
                    continue  # run_make_test(undef, ...) reuses the previous makefile
                makefile = rec['makefile']
                answer = rec['answer'] or ''
                code = rec['err_code'] or 0
                expect = 'reject' if MAKE_READ_ERRORS.search(answer) and code else 'accept'
                raw = rec['raw']
                verbatim = makefile in (raw, raw + '\n') and raw in source
                name = f'{script.replace("/", "-")}-{index:02}'
                path = f'admission/cases/make/harness/{name}.mk'
                (ROOT / path).write_bytes(makefile.encode('utf-8'))
                documents.append({
                    'id': f'gmake-{name}', 'path': path, 'script': script, 'index': index, 'line': rec['line'],
                    'options': rec['options'] or '', 'answer': answer, 'err_code': code, 'expect': expect,
                    'harness_passed': bool(rec['passed']), 'verbatim': verbatim,
                    'derivation': ('makefile string of the run_make_test call, written by the upstream harness '
                                   'unchanged' if verbatim else
                                   'makefile written by the upstream harness: run_make_test appends a final newline '
                                   'and substitutes #MAKEFILE#/#MAKE#/#PWD#-style tokens, and Perl string quoting '
                                   'applies')})
                index += 1
            print(f'make {script}: {index} makefiles (harness exit {res.returncode})')
    index_path = out_dir / 'index.json'
    index_path.write_text(json.dumps({'tool': 'scripts/build_infra_witness.py extract', 'make': ENGINES.get('make'),
                                      'perl': ENGINES.get('perl'), 'documents': documents},
                                     indent=2, sort_keys=True, ensure_ascii=False) + '\n')


def extract_puppet():
    out_dir = ROOT / 'admission/cases/puppet/specs'
    if out_dir.exists():
        shutil.rmtree(out_dir)
    out_dir.mkdir(parents=True)
    documents, seen = [], set()
    specs = UP / 'puppet-parser-specs' / 'spec/unit/pops/parser'
    for spec in sorted(specs.glob('*_spec.rb')):
        res = run(['ruby', str(PUPPET_EXTRACT), str(spec)])
        if res.returncode:
            raise RuntimeError(res.stderr.decode())
        for line in res.stdout.decode().splitlines():
            rec = json.loads(line)
            if rec['expectation'] not in ('accept', 'reject') or rec['value'] in seen or not rec['value'].strip():
                continue
            seen.add(rec['value'])
            stem = spec.stem.removesuffix('_spec')
            path = f'admission/cases/puppet/specs/{stem}-L{rec["line"]:04}.pp'
            while (ROOT / path).exists():
                path = path[:-3] + 'b.pp'
            (ROOT / path).write_bytes(rec['value'].encode('utf-8'))
            verbatim = rec['raw'] == rec['value']
            documents.append({
                'id': f'puppet-{stem.replace("_", "-")}-l{rec["line"]:04}' + ('b' if path.endswith('b.pp') else ''),
                'path': path, 'spec': spec.name, 'line': rec['line'], 'value': rec['value'], 'quote': rec['quote'],
                'expect': rec['expectation'], 'verbatim': verbatim,
                'derivation': ('string literal argument of parse(), extracted byte-exact (no escape sequences)'
                               if verbatim else 'string literal argument of parse() with its Ruby escape sequences '
                               'evaluated (no interpolation)')})
    (out_dir / 'index.json').write_text(json.dumps({'tool': 'scripts/witness_tools/puppet/extract_specs.rb (Ripper)',
                                                    'documents': documents}, indent=2, sort_keys=True,
                                                   ensure_ascii=False) + '\n')
    print(f'puppet: {len(documents)} manifests')


# ------------------------------------------------------------------ oracles

def w_buildkit_parse(case):
    r = tool_json('dockerfile', ROOT / case['old']['path'])
    plan = case['plan']
    obs = {'tool': r['tool'], 'reference_accepts': r['accepts'], 'unit_starts': r['unit_starts'],
           'reference_error': r.get('error')}
    if plan['expect'] == 'reject':
        ok = not r['accepts']
        return write_record(case, 'supports_claim' if ok else 'contradicts_claim',
                            'parser.Parse returned an error, as TestParseErrorCases requires.' if ok else
                            'parser.Parse accepted a must-reject file.', 'buildkit_parse', ['buildinfra'], **obs)
    if not r['accepts']:
        return write_record(case, 'contradicts_claim', 'parser.Parse rejected the file: ' + r['error'],
                            'buildkit_parse', ['buildinfra'], **obs)
    if plan['result']:
        expected = (ROOT / plan['result']).read_text(encoding='utf-8')
        same = r['projection']['dump'] == expected
        obs['upstream_result_reproduced'] = same
        if not same:
            return write_record(case, 'inconclusive', 'AST dump differs from the upstream result file.',
                                'buildkit_parse', ['buildinfra'], **obs)
    return write_record(case, 'supports_claim', 'parser.Parse accepts the file' + (
        ' and AST.Dump() reproduces the upstream result file byte for byte.' if plan['result'] else '.'),
        'buildkit_parse', ['buildinfra'], **obs)


def compare_pair(case, oracle, a, b, name, used, domain):
    if a is None or b is None:
        return write_record(case, 'inconclusive', 'the reference did not accept both sides', oracle, used,
                            old_observation=a, new_observation=b)
    equal = a == b
    verdict = 'supports_claim' if (equal and case['claim'] == 'preserved') or (not equal and case['claim'] == 'changed') \
        else 'contradicts_claim'
    review = f"{name}: {domain} {'equal' if equal else 'differ'} on old and new"
    return write_record(case, verdict, review + '.', oracle, used, projection_equal=equal,
                        old_observation=a, new_observation=b)


def w_buildkit_pair(case):
    sides = []
    for side in ('old', 'new'):
        r = tool_json('dockerfile', ROOT / case[side]['path'])
        p = r.get('projection') or {}
        sides.append(None if not r['accepts'] or 'instructions_error' in p else
                     {'stages': p['stages'], 'meta_args': p['meta_args'], 'escape_token': p['escape_token']})
    return compare_pair(case, 'buildkit_instructions_equality', *sides, 'BuildKit instructions.Parse',
                        ['buildinfra'], 'the typed instruction lists')


def w_hcl_parse(case):
    r = tool_json('hcl', ROOT / case['old']['path'])
    plan = case['plan']
    obs = {'tool': r['tool'], 'reference_accepts': r['accepts'], 'unit_starts': r['unit_starts'],
           'reference_error': r.get('error')}
    label = plan.get('upstream_label')
    if label == 'valid-files' and not r['accepts']:
        return write_record(case, 'contradicts_claim', 'a valid-files configuration is rejected by hclsyntax: '
                            + r['error'], 'hcl_native_syntax', ['buildinfra'], **obs)
    if plan.get('upstream_expects_error') is False and not r['accepts']:
        return write_record(case, 'contradicts_claim', 'the spec test expects no diagnostics, but hclsyntax '
                            'rejects the document', 'hcl_native_syntax', ['buildinfra'], **obs)
    review = ('hclsyntax.ParseConfig accepts the document (native syntax).' if r['accepts'] else
              'hclsyntax.ParseConfig reports syntax errors: ' + r['error'][:300])
    if label:
        review += f' Upstream directory label: {label}.'
    if plan.get('upstream_expects_error'):
        review += ' The upstream .t expects error diagnostics.'
    return write_record(case, 'supports_claim', review, 'hcl_native_syntax', ['buildinfra'], **obs)


def w_hcl_pair(case):
    sides = []
    for side in ('old', 'new'):
        r = tool_json('hcl', ROOT / case[side]['path'])
        sides.append(r['projection'] if r['accepts'] else None)
    return compare_pair(case, 'hcl_information_model_equality', *sides, 'hclsyntax v2.23.0', ['buildinfra'],
                        'the information-model projections')


def gomod_json(path):
    with tempfile.TemporaryDirectory(prefix='gomod-') as tmp:
        shutil.copyfile(path, Path(tmp) / 'go.mod')
        env = dict(os.environ, GOTOOLCHAIN='local', GOWORK='off', GOFLAGS='', GOPROXY='off', GO111MODULE='on')
        res = run([GO, 'mod', 'edit', '-json'], cwd=tmp, env=env)
        if res.returncode:
            return None, res.stderr.decode(errors='replace').replace(tmp, '<TMP>').strip()
        return json.loads(res.stdout), None


def w_modfile_parse(case):
    path = ROOT / case['old']['path']
    r = tool_json('modfile', path)
    js, err = gomod_json(path)
    obs = {'tool': r['tool'], 'reference_accepts': r['accepts'] and js is not None, 'unit_starts': r['unit_starts'],
           'xmod_error': r.get('error'), 'cmd_go_error': err}
    if r['accepts'] and js is not None:
        return write_record(case, 'supports_claim', 'modfile.Parse and `go mod edit -json` both accept the file.',
                            'gomod_parse', ['buildinfra', 'go'], **obs)
    return write_record(case, 'contradicts_claim', 'the reference rejects the file: ' + (r.get('error') or err or ''),
                        'gomod_parse', ['buildinfra', 'go'], **obs)


def w_modfile_pair(case):
    a, ea = gomod_json(ROOT / case['old']['path'])
    b, eb = gomod_json(ROOT / case['new']['path'])
    return compare_pair(case, 'go_mod_edit_json_equality', a, b, 'cmd/go `go mod edit -json`', ['go'],
                        'the module JSON documents')


# git ------------------------------------------------------------------------------

def git_env(home):
    return dict(os.environ, HOME=home, GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull, LC_ALL='C',
                GIT_AUTHOR_NAME='w', GIT_AUTHOR_EMAIL='w@example.invalid', GIT_COMMITTER_NAME='w',
                GIT_COMMITTER_EMAIL='w@example.invalid')


SETUP_FILES = {'.gitignore': 'setup-gitignore', 'a/.gitignore': 'setup-a-gitignore',
               'a/b/.gitignore': 'setup-a-b-gitignore', 'global-excludes': 'setup-global-excludes',
               '.git/info/exclude': 'setup-info-exclude'}


def git_repo(tmp, files):
    env = git_env(tmp)
    repo = Path(tmp) / 'repo'
    repo.mkdir()
    run([GIT, 'init', '-q', str(repo)], env=env, check=True)
    for rel, text in files.items():
        target = repo / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(text)
    return repo, env


def t0008_text():
    return (UP / family.T0008).read_text(encoding='utf-8')


def unquoted_heredoc(script, target):
    """`cat <<-EOF >target`: leading tabs removed, then the shell's backslash/$ expansion of t0008's values."""
    lines = script.splitlines()
    start = next(i for i, l in enumerate(lines) if re.match(rf'^\t*cat <<-EOF >{re.escape(target)}$', l))
    body = []
    for line in lines[start + 1:]:
        if re.match(r'^\t*EOF$', line):
            break
        line = line.lstrip('\t').replace('$global_excludes', 'global-excludes')
        body.append(re.sub(r'\\([\\$`])', r'\1', line) + '\n')
    return ''.join(body)


def setup_payloads():
    out = {}
    for rel, name in SETUP_FILES.items():
        out[rel] = (ROOT / f'admission/cases/gitignore/t0008/{name}').read_bytes()
    out['a/b/ignored-dir/.gitignore'] = b'seven\n'  # `echo "seven" >a/b/ignored-dir/.gitignore` in setup
    return out


def run_t0008(test):
    script = t0008_text()
    with tempfile.TemporaryDirectory(prefix='t0008-') as tmp:
        if test == 'setup':
            repo, env = git_repo(tmp, setup_payloads())
            stdin = next(h for h in family.heredocs(script) if h['target'] == 'stdin')['payload']
            expected = unquoted_heredoc(script, 'expected-verbose')
            res = run([GIT, '-c', 'core.excludesfile=global-excludes', 'check-ignore', '--stdin', '-v'], cwd=repo,
                      env=env, input=stdin.encode())
            actual = res.stdout.decode()
            return actual == expected, {'command': 'git -c core.excludesfile=global-excludes check-ignore --stdin -v',
                                        'stdin': stdin, 'expected': expected, 'actual': actual,
                                        'exit_status': res.returncode}
        if test == 'directories and ** matches':
            cmd = 'git check-ignore file data/file data/data1/file1 data/data1/file1.txt data/data2/file2 data/data2/file2.txt'
            if 'git check-ignore file \\\n\t\tdata/file data/data1/file1 data/data1/file1.txt \\\n\t\tdata/data2/file2 data/data2/file2.txt' not in script:
                raise RuntimeError('t0008 ** test command anchor not found')
            expected = 'data/file\ndata/data1/file1\ndata/data2/file2\n'
            if f"cat >expect <<-\\EOF &&\n\t{expected.replace(chr(10), chr(10) + chr(9)).rstrip(chr(9))}\tEOF" not in script:
                raise RuntimeError('t0008 ** test expectation anchor not found')
            repo, env = git_repo(tmp, {'.gitignore': (ROOT / 'admission/cases/gitignore/t0008/dirs-gitignore').read_bytes()})
            res = run(shlex.split(cmd), cwd=repo, env=env)
            return res.stdout.decode() == expected, {'command': cmd, 'expected': expected,
                                                    'actual': res.stdout.decode(), 'exit_status': res.returncode}
        if test == 'correct handling of backslashes':
            section = script[script.index("'correct handling of backslashes'"):]
            section = section[:section.index("\n'\n")]
            # `>"whitespace/..."` lines of the single-quoted test body; inside the shell's double quotes each
            # pair of backslashes becomes one.
            names = [m.replace('\\\\', '\\') for m in re.findall(r'^\t>"(whitespace/[^"]*)" &&$', section, re.M)]
            names += re.findall(r'^\t>(whitespace/untracked) &&$', section, re.M)
            if len(names) != 7:
                raise RuntimeError(f't0008 backslash test: expected 7 file names, found {names}')
            repo, env = git_repo(tmp, {'ignore': (ROOT / 'admission/cases/gitignore/t0008/whitespace-ignore').read_bytes()})
            for name in names:
                p = repo / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(b'')
            res = run([GIT, 'ls-files', '-o', '-X', 'ignore', 'whitespace'], cwd=repo, env=env)
            return (res.stdout.decode() == 'whitespace/untracked\n' and not res.stderr), {
                'command': 'git ls-files -o -X ignore whitespace', 'expected': 'whitespace/untracked\n',
                'actual': res.stdout.decode(), 'stderr': res.stderr.decode(), 'exit_status': res.returncode}
    raise ValueError(test)


T0008_CACHE = {}


def w_git_t0008(case):
    test = case['plan']['test']
    if test not in T0008_CACHE:
        T0008_CACHE[test] = run_t0008(test)
    ok, obs = T0008_CACHE[test]
    return write_record(case, 'supports_claim' if ok else 'inconclusive',
                        f"git reproduces the t0008 `{test}` expectation with this file in place." if ok else
                        f"the t0008 `{test}` expectation was not reproduced.", 'git_t0008_reproduction', ['git'],
                        reference_accepts=True, t0008=obs)


def ignored_set(case, side):
    plan = case['plan']
    text = data(case[side])
    with tempfile.TemporaryDirectory(prefix='gitpair-') as tmp:
        if plan['layout'] == 'setup':
            files = setup_payloads()
            key = 'global-excludes' if plan['target'] == '"$global_excludes"' else plan['target']
            files[key] = text
        else:
            files = {'.gitignore': text}
        repo, env = git_repo(tmp, files)
        for p in plan['universe']:
            target = repo / p
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(b'')
        res = run([GIT, '-c', 'core.excludesfile=global-excludes', 'check-ignore', '--no-index', '-z', '--stdin'],
                  cwd=repo, env=env, input=b'\0'.join(p.encode() for p in plan['universe']) + b'\0')
        if res.returncode not in (0, 1):
            return None
        return sorted(x for x in res.stdout.decode().split('\0') if x)


def w_git_pair(case):
    return compare_pair(case, 'git_check_ignore_membership', ignored_set(case, 'old'), ignored_set(case, 'new'),
                        'git check-ignore --no-index', ['git'], 'the ignored path sets')


# cmake ----------------------------------------------------------------------------

RC = UP / 'cmake-runcmake-syntax' / 'Tests' / 'RunCMake'


def w_runcmake(case):
    name = case['plan']['test']
    with tempfile.TemporaryDirectory(prefix='runcmake-') as tmp:
        driver = Path(tmp) / 'driver.cmake'
        driver.write_text('include(RunCMake)\nset(RunCMake_IGNORE_POLICY_VERSION_DEPRECATION ON)\n'
                          f'run_cmake({name})\n')
        res = run([CMAKE, '-DRunCMake_GENERATOR=Unix Makefiles', f'-DRunCMake_SOURCE_DIR={RC / "Syntax"}',
                   f'-DRunCMake_BINARY_DIR={tmp}/out', f'-DCMAKE_MODULE_PATH={RC}',
                   f'-DRunCMake_MAKE_PROGRAM={shutil.which(MAKE) or MAKE}', '-P', str(driver)], cwd=tmp,
                  env=dict(os.environ, LC_ALL='C'))
        log = (res.stdout + res.stderr).decode(errors='replace').replace(tmp, '<TMP>').replace(str(HERE.parent), '<REPO>')
    passed = res.returncode == 0 and f'-- {name} - PASSED' in log
    obs = {'reference_accepts': case['plan']['expect'] == 'accept', 'harness': 'Tests/RunCMake/RunCMake.cmake '
           'run_cmake() with RunCMakeTest.cmake preamble (RunCMake_IGNORE_POLICY_VERSION_DEPRECATION ON)',
           'harness_passed': passed, 'harness_log_tail': log[-1500:] if not passed else None}
    return write_record(case, 'supports_claim' if passed else 'inconclusive',
                        (f'The RunCMake harness reproduces the upstream result/stdout/stderr expectation for '
                         f'run_cmake({name}) on cmake 3.28.3' + ('; that expectation is a parse error in this file.'
                                                                  if case['plan']['expect'] == 'reject' else '.'))
                        if passed else f'run_cmake({name}) did not pass the harness (missing helper inputs or a '
                        'version difference).', 'runcmake_harness', ['cmake', 'make'], **obs)


def cmake_observe(ref):
    with tempfile.TemporaryDirectory(prefix='cmakepair-') as tmp:
        src = Path(tmp) / 'src'
        src.mkdir()
        shutil.copyfile(RC / 'Syntax' / 'CMakeLists.txt', src / 'CMakeLists.txt')
        shutil.copyfile(ROOT / ref['path'], src / 'Case.cmake')
        res = run([CMAKE, '-S', str(src), '-B', f'{tmp}/build', '-G', 'Unix Makefiles', '-DRunCMake_TEST=Case',
                   '-Wno-dev', '-Wno-deprecated'], env=dict(os.environ, LC_ALL='C'))

        def norm(b):
            return re.sub(r'\(\d+\.\d+s\)', '(Ts)', b.decode(errors='replace').replace(tmp, '<TMP>'))
        return {'exit_status': res.returncode, 'stdout': norm(res.stdout), 'stderr': norm(res.stderr)}


def w_cmake_pair(case):
    return compare_pair(case, 'cmake_configure_output_equality', cmake_observe(case['old']), cmake_observe(case['new']),
                        'cmake 3.28.3 configure', ['cmake'], 'exit status, stdout and stderr')


# make -----------------------------------------------------------------------------

def w_make_harness(case):
    e = case['plan']['entry']
    ok = e['harness_passed']
    return write_record(case, 'supports_claim' if ok else 'inconclusive',
                        ('The upstream run_make_tests.pl harness, run on GNU Make 4.3, reproduced the expected output '
                         'and exit status of this run_make_test call' + (
                             ', which is a makefile read error.' if e['expect'] == 'reject' else '.')) if ok else
                        'The upstream harness did not reproduce the expectation in this environment.',
                        'gnu_make_harness', ['make', 'perl'], reference_accepts=e['expect'] == 'accept',
                        harness={'script': e['script'], 'line': e['line'], 'options': e['options'],
                                 'expected_output': e['answer'], 'expected_exit_status': e['err_code']})


def make_observe(ref, options):
    with tempfile.TemporaryDirectory(prefix='makepair-') as tmp:
        shutil.copyfile(ROOT / ref['path'], Path(tmp) / 'Makefile.case')
        res = run([MAKE, '-f', 'Makefile.case', *shlex.split(options or '')], cwd=tmp, env={
            'PATH': os.environ.get('PATH', ''), 'LC_ALL': 'C', 'HOME': tmp})
        return {'exit_status': res.returncode, 'stdout': res.stdout.decode(errors='replace').replace(tmp, '<TMP>'),
                'stderr': res.stderr.decode(errors='replace').replace(tmp, '<TMP>')}


def w_make_pair(case):
    opts = case['plan']['options']
    return compare_pair(case, 'gnu_make_output_equality', make_observe(case['old'], opts),
                        make_observe(case['new'], opts), 'GNU Make 4.3', ['make'], 'exit status, stdout and stderr')


# puppet ---------------------------------------------------------------------------

def puppet(mode, ref):
    res = run([RUBY, str(PUPPET_ORACLE), mode, str(ROOT / ref['path'])], timeout=300)
    if res.returncode:
        raise RuntimeError(res.stderr.decode(errors='replace')[:500])
    return json.loads(res.stdout)


def w_puppet_parse(case):
    r = puppet('parse', case['old'])
    want = case['plan']['expect'] == 'accept'
    ok = r['accepts'] == want
    return write_record(case, 'supports_claim' if ok else 'contradicts_claim',
                        f"Puppet {r['tool'].split()[-1]} Parser#parse_string " + ('accepts' if r['accepts'] else
                                                                                 'raises: ' + r.get('error', '')[:300])
                        + (', as the spec expects.' if ok else ', contrary to the spec.'), 'puppet_parse', ['puppet'],
                        reference_accepts=r['accepts'], reference_error=r.get('error'))


def w_puppet_pair(case):
    a, b = puppet('catalog', case['old']), puppet('catalog', case['new'])
    return compare_pair(case, 'puppet_catalog_equality', a.get('projection') if a['accepts'] else None,
                        b.get('projection') if b['accepts'] else None, 'Puppet 8.10.0 catalog', ['puppet'],
                        'the normalised catalogs')


ORACLES = {'buildkit-parse': w_buildkit_parse, 'buildkit-pair': w_buildkit_pair, 'hcl-parse': w_hcl_parse,
           'hcl-pair': w_hcl_pair, 'modfile-parse': w_modfile_parse, 'modfile-pair': w_modfile_pair,
           'git-t0008': w_git_t0008, 'git-pair': w_git_pair, 'runcmake': w_runcmake, 'cmake-pair': w_cmake_pair,
           'make-harness': w_make_harness, 'make-pair': w_make_pair, 'puppet-parse': w_puppet_parse,
           'puppet-pair': w_puppet_pair}


def writer(path, text):
    target = ROOT / path
    data = text.encode('utf-8')
    if not target.exists() or target.read_bytes() != data:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    return {'path': path, 'sha256': sha(data)}


def witness(only=None, jobs=8):
    cases = family.plan(ROOT, writer)
    for c in cases:
        if c['new'] is None:  # canonical formatting produced by the reference formatter
            _, src, out = c['new_from_tool']
            try:
                res = run([TOOL, 'hclfmt', str(ROOT / src)])
            except FileNotFoundError:
                print(f"blocked: {c['id']}: the buildinfra tool is not available, so no formatted bytes")
                c['skip'] = True
                continue
            if res.returncode:
                raise RuntimeError(res.stderr.decode())
            if res.stdout == (ROOT / src).read_bytes():
                c['skip'] = True  # already canonical: no pair
                continue
            (ROOT / out).parent.mkdir(parents=True, exist_ok=True)
            (ROOT / out).write_bytes(res.stdout)
            c['new'] = {'path': out, 'sha256': sha(res.stdout)}
    todo = [c for c in cases if not c.get('skip') and (only is None or re.search(only, c['id']))]
    if only is None and WIT.exists():
        shutil.rmtree(WIT)
    def one(c):
        try:
            return c['id'], ORACLES[c['plan']['oracle']](c)
        except FileNotFoundError as error:  # a reference tool is not installed: never admit
            return c['id'], write_record(c, 'blocked', f'reference tool not available: {error}',
                                         c['plan']['oracle'], [])

    with ThreadPoolExecutor(jobs) as pool:
        verdicts = list(pool.map(one, todo))
    summary = {}
    for cid, verdict in verdicts:
        summary[verdict] = summary.get(verdict, 0) + 1
    print(json.dumps(summary, sort_keys=True))
    for cid, verdict in verdicts:
        if verdict != 'supports_claim':
            print(verdict, cid)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('command', choices=('extract', 'witness'))
    parser.add_argument('--only', help='regex of case ids (witness)')
    parser.add_argument('--jobs', type=int, default=8)
    args = parser.parse_args()
    ENGINES.update(engines())
    if args.command == 'extract':
        extract_make(); extract_puppet()
    else:
        witness(args.only, args.jobs)
    return 0


if __name__ == '__main__':
    sys.exit(main())
