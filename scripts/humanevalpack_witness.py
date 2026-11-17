#!/usr/bin/env python3
"""Execute retained HumanEvalPack tests against exact admitted case bytes (#261/#268).

This is benchmark tooling outside the runtime engine. It establishes the external
behavior oracle for a case, never IntentumDiff's answer: the upstream test is run
with an explicit toolchain against the exact old/new source bytes recorded in the
admission ledger. A test outcome is bounded evidence for the exercised inputs; it
is not program equivalence. No network I/O: Go and Rust dependencies must already
be present in the supplied harness directories (``--go-harness``/``--rust-harness``).

Witness records are written as JSON and referenced (path + SHA-256) from the
ledger. Missing toolchains are recorded as ``blocked`` instead of silently passing.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

TIMEOUT_SECONDS = 60


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _version(argv):
    try:
        out = subprocess.run(argv, capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = (out.stdout + out.stderr).strip().splitlines()
    text = [line for line in text if not line.startswith('Picked up JAVA_TOOL_OPTIONS')]
    return text[0] if text else None


TOOLCHAINS = {
    'python': (['python3', '--version'],),
    'js': (['node', '--version'],),
    'java': (['javac', '-version'], ['java', '-version']),
    'go': (['go', 'version'],),
    'cpp': (['g++', '--version'],),
    'c': (['gcc', '--version'],),
    'rust': (['cargo', '--version'], ['rustc', '--version']),
}


def toolchain(config):
    versions = [_version(argv) for argv in TOOLCHAINS[config]]
    return None if any(v is None for v in versions) else versions


def _run(argv, cwd, timeout=TIMEOUT_SECONDS, env=None):
    try:
        out = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, timeout=timeout,
                             env={**os.environ, **(env or {})})
        return {'argv': argv, 'exit_code': out.returncode, 'timed_out': False,
                'stdout_tail': out.stdout[-1500:], 'stderr_tail': out.stderr[-1500:]}
    except subprocess.TimeoutExpired as error:
        def tail(v):
            return (v.decode(errors='replace') if isinstance(v, bytes) else (v or ''))[-1500:]
        return {'argv': argv, 'exit_code': None, 'timed_out': True,
                'stdout_tail': tail(error.stdout), 'stderr_tail': tail(error.stderr)}


def _outcome(steps, assertion_marker=None):
    """``passed`` only when every step exits 0 (and no soft assertion fired)."""
    last = steps[-1]
    if any(s['timed_out'] for s in steps):
        return 'timeout'
    for s in steps[:-1]:
        if s['exit_code'] != 0:
            return 'build_failed'
    if last['exit_code'] != 0:
        # `go test` compiles and runs in one step: a compile error is not a test failure.
        if '[build failed]' in last['stdout_tail'] or '[setup failed]' in last['stdout_tail']:
            return 'build_failed'
        return 'failed'
    if assertion_marker and assertion_marker in last['stderr_tail'] + last['stdout_tail']:
        return 'failed'
    return 'passed'


def execute(config, source: str, test: str, test_setup: str, harness: dict) -> dict:
    """Run ``test`` against ``source`` exactly; returns steps and an outcome."""
    with tempfile.TemporaryDirectory(prefix='hep-witness-') as tmp:
        work = Path(tmp)
        marker = None
        if config == 'python':
            program = source + '\n' + test + '\n'
            (work / 'prog.py').write_text(program)
            steps = [_run(['python3', '-I', 'prog.py'], work)]
        elif config == 'js':
            program = source + '\n' + test + '\n'
            (work / 'prog.js').write_text(program)
            steps = [_run(['node', 'prog.js'], work)]
            marker = 'Assertion failed'  # console.assert reports but does not exit nonzero
        elif config == 'java':
            program = source + '\n' + test + '\n'
            (work / 'Main.java').write_text(program)
            steps = [_run(['javac', 'Main.java'], work)]
            if steps[-1]['exit_code'] == 0:
                steps.append(_run(['java', '-ea', 'Main'], work))
        elif config in ('cpp', 'c'):
            ext, compiler, std = ('cpp', 'g++', '-std=c++17') if config == 'cpp' else ('c', 'gcc', '-std=c11')
            program = source + '\n' + test + '\n'
            (work / f'prog.{ext}').write_text(program)
            steps = [_run([compiler, std, '-O0', f'prog.{ext}', '-o', 'prog', '-lm'], work)]
            if steps[-1]['exit_code'] == 0:
                steps.append(_run(['./prog'], work))
        elif config == 'go':
            base = Path(harness['go'])
            for name in ('go.mod', 'go.sum'):
                shutil.copy(base / name, work / name)
            (work / 'main.go').write_text(source)
            program = test_setup + '\n' + test + '\n'
            (work / 'main_test.go').write_text(program)
            # `go test` runs vet by default; disable it so only compilation and the test gate.
            steps = [_run(['go', 'test', '-vet=off', '-count=1', './...'], work,
                          env={'GOFLAGS': '-mod=mod', 'GOPROXY': 'off'})]
        elif config == 'rust':
            base = Path(harness['rust'])
            crate = work / 'crate'
            (crate / 'src').mkdir(parents=True)
            for name in ('Cargo.toml', 'Cargo.lock'):
                shutil.copy(base / name, crate / name)
            program = 'fn main() {}\n' + source + '\n' + test + '\n'
            (crate / 'src' / 'main.rs').write_text(program)
            env = {'CARGO_TARGET_DIR': str(Path(harness['rust']) / 'target'), 'CARGO_NET_OFFLINE': 'true',
                   'CARGO_INCREMENTAL': '0', 'RUSTFLAGS': '-Awarnings'}
            steps = [_run(['cargo', 'test', '--offline', '--quiet', '--no-run'], crate, timeout=600, env=env)]
            if steps[-1]['exit_code'] == 0:
                steps.append(_run(['cargo', 'test', '--offline', '--quiet'], crate, env=env))
        else:
            raise ValueError(f'unsupported witness config {config}')
        return {'program_sha256': sha(program.encode()), 'steps': steps,
                'outcome': _outcome(steps, marker)}


def witness_case(config, old: str, new: str, old_test: str, new_test: str, test_setup: str, harness, versions):
    if versions is None:
        return {'status': 'blocked', 'reason': f'toolchain for {config} not available', 'old': None, 'new': None}
    old_run = execute(config, old, old_test, test_setup, harness)
    new_run = execute(config, new, new_test, test_setup, harness)
    return {'status': 'executed', 'old': old_run, 'new': new_run}


def verdict(record, claim):
    """Map executed outcomes to an oracle verdict for a behavior claim."""
    if record['status'] != 'executed':
        return 'blocked'
    old, new = record['old']['outcome'], record['new']['outcome']
    if 'build_failed' in (old, new):
        return 'inconclusive'  # a harness/compile problem is not behavioral evidence
    if claim == 'changed':
        # The canonical side must pass; the buggy side must fail its assertion or time out.
        return 'supports_claim' if new == 'passed' and old != 'passed' else 'contradicts_claim'
    if claim == 'preserved':
        return 'supports_claim' if old == new == 'passed' else 'contradicts_claim'
    return 'inconclusive'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=Path('tests/fixtures/external'))
    parser.add_argument('--ledger', type=Path, default=None,
                        help='admission ledger (default: <fixtures>/admission/ledger.json)')
    parser.add_argument('--go-harness', type=Path, required=True)
    parser.add_argument('--rust-harness', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True, help='directory for witness records')
    parser.add_argument('--cases', nargs='*', help='only these case IDs')
    args = parser.parse_args()
    root = args.fixtures.resolve()
    ledger_path = args.ledger or root / 'admission' / 'ledger.json'
    ledger = json.loads(ledger_path.read_text())
    harness = {'go': str(args.go_harness.resolve()), 'rust': str(args.rust_harness.resolve())}
    versions = {}
    args.out.mkdir(parents=True, exist_ok=True)
    for case in ledger['cases']:
        plan = case.get('witness_plan')
        if not plan or (args.cases and case['id'] not in args.cases):
            continue
        config = plan['config']
        if config not in versions:
            versions[config] = toolchain(config)
        old = (root / case['old']['path']).read_bytes()
        new = (root / case['new']['path']).read_bytes()
        if sha(old) != case['old']['sha256'] or sha(new) != case['new']['sha256']:
            raise SystemExit(f"{case['id']}: case bytes differ from ledger")
        if 'inline_test' in plan:
            row = {'test': plan['inline_test'], 'test_setup': ''}
            source_note = 'inline witness test recorded in the ledger witness_plan'
        else:
            record_bytes = (root / plan['upstream_record']['path']).read_bytes()
            if sha(record_bytes) != plan['upstream_record']['sha256']:
                raise SystemExit(f"{case['id']}: upstream record differs from ledger")
            row = json.loads(record_bytes)
            source_note = 'upstream `test` field of ' + plan['upstream_record']['path']
        old_test = new_test = row['test']
        for rule in plan.get('new_test_substitutions', []):
            # e.g. a public rename: callers (the test) follow the renamed binding on the new side only.
            new_test = re.sub(rule['pattern'], rule['replacement'], new_test)
        record = witness_case(config, old.decode(), new.decode(), old_test, new_test,
                              row.get('test_setup') or '', harness, versions[config])
        record.update({'schema_version': 1, 'case_id': case['id'], 'config': config,
                       'toolchain': versions[config], 'old_sha256': case['old']['sha256'],
                       'new_sha256': case['new']['sha256'], 'test_source': source_note,
                       'old_test_sha256': sha(old_test.encode()), 'new_test_sha256': sha(new_test.encode()),
                       'new_test_substitutions': plan.get('new_test_substitutions', []),
                       'behavior_claim': case['behavior']['claim']})
        record['verdict'] = verdict(record, case['behavior']['claim'])
        target = args.out / f"{case['id']}.json"
        target.write_text(json.dumps(record, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
        print(case['id'], record['status'], (record.get('old') or {}).get('outcome'),
              (record.get('new') or {}).get('outcome'), record['verdict'], flush=True)


if __name__ == '__main__':
    main()
