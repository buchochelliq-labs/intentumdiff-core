#!/usr/bin/env python3
"""Acquire the #267 build, configuration and infrastructure suites at immutable revisions.

Benchmark tooling only (core #267 / #268). Two acquisition methods are used, both at an
immutable commit:

- ``raw``: ``raw.githubusercontent.com/<owner>/<repo>/<40-hex commit>/<path>``. The commit
  was resolved from a tag or branch with the Go module proxy's VCS origin record
  (``proxy.golang.org/<repo>/@v/<ref>.info`` -> ``Origin.Hash``), because github.com HTML,
  the REST API, ``git ls-remote`` and git clones are not reachable from the sandbox.
- ``gomod``: a Go module zip downloaded with ``go mod download -json``, which verifies the
  zip against the public checksum database (sum.golang.org). The manifest records the
  zip SHA-256, the ``h1:`` module sum and the module's VCS origin commit.

Each suite writes its selected files verbatim under
``tests/fixtures/external/upstream/build-infra/<suite_id>/`` with an ``ACQUISITION.json``
manifest (source URL, SHA-256 and size per file, and what could and could not be verified).
Directory listings are not available, so each suite is enumerated from an index that is
itself part of the suite at the same revision (RunCMake ``RunCMakeTest.cmake``, a module
zip's own file list) or from the Go toolchain installed locally at the same release, whose
file bytes are then re-fetched and compared at the pinned commit.

    python scripts/acquire_build_infra_benchmarks.py [--suite ID ...]   # network
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import zipfile
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures' / 'external' / 'upstream' / 'build-infra'
RAW = 'https://raw.githubusercontent.com'

PUPPET_SPECS = ('parse_basic_expressions_spec.rb', 'parse_resource_spec.rb', 'parse_conditionals_spec.rb',
                'parse_calls_spec.rb', 'parse_containers_spec.rb', 'parse_heredoc_spec.rb',
                'parse_lambda_spec.rb', 'parse_functions_spec.rb', 'parser_spec.rb')
MAKE_SCRIPTS = ('variables/flavors', 'variables/define', 'variables/automatic', 'variables/negative',
                'variables/special', 'features/order_only', 'features/conditionals', 'features/targetvars',
                'features/comments', 'features/escape', 'functions/foreach')

SUITES = {
    'git-t0008-ignores': {
        'kind': 'raw', 'repo': 'git/git', 'ref': 'v2.43.0', 'revision': '564d0252ca632e0264ed670534a51d18a689ef5d',
        'files': ['t/t0008-ignores.sh', 'COPYING']},
    'go-cmd-mod-testdata': {
        'kind': 'raw', 'repo': 'golang/go', 'ref': 'go1.24.7', 'revision': 'd41a8222133aabaf5527eab64586050a0fbb773b',
        'files': ['LICENSE', 'VERSION', 'src/cmd/go/testdata/script/mod_replace.txt'],
        'local_index': ('/usr/local/go', 'src/cmd/go/testdata/mod')},
    'x-mod-modfile': {
        'kind': 'gomod', 'module': 'golang.org/x/mod', 'version': 'v0.22.0',
        'revision': 'dec0365065b75edd0e98b0306f6f9b0051710ed2',
        'select': lambda p: p == 'LICENSE' or p.startswith('modfile/testdata/')},
    'buildkit-dockerfile-parser': {
        'kind': 'gomod', 'module': 'github.com/moby/buildkit', 'version': 'v0.17.3',
        'revision': 'dc30e8355a4691bb34a23dd7161306977eecd5d8',
        'select': lambda p: p in ('LICENSE', 'NOTICE') or p.startswith('frontend/dockerfile/parser/testfile')},
    'hcl-specsuite': {
        'kind': 'gomod', 'module': 'github.com/hashicorp/hcl/v2', 'version': 'v2.23.0',
        'revision': '56a9aee5207dbaed7f061cd926b96fc159d26ea0',
        'select': lambda p: p == 'LICENSE' or p.startswith('specsuite/tests/')},
    'terraform-configs-testdata': {
        'kind': 'gomod', 'module': 'github.com/hashicorp/terraform', 'version': 'v1.5.7',
        'revision': 'ee58ac1851c8a433005df9863ed47796a9f6b5e7',
        'select': lambda p: p == 'LICENSE' or re.fullmatch(r'internal/configs/testdata/(valid|invalid)-files/[^/]+', p)},
    'cmake-runcmake-syntax': {
        'kind': 'raw', 'repo': 'Kitware/CMake', 'ref': 'v3.28.3', 'revision': '5e984bb35232116a54de7db39788cb162ca7c263',
        'files': ['Copyright.txt', 'Tests/RunCMake/RunCMake.cmake', 'Tests/RunCMake/Syntax/RunCMakeTest.cmake',
                  'Tests/RunCMake/Syntax/CMakeLists.txt'],
        'runcmake_dir': 'Tests/RunCMake/Syntax'},
    'gnu-make-tests': {
        'kind': 'raw', 'repo': 'mirror/make', 'ref': '4.3', 'revision': 'f430a65ccb5e85eb465f6a64afcbdd09f9c708ed',
        'files': ['COPYING', 'tests/run_make_tests.pl', 'tests/test_driver.pl', 'tests/config-flags.pm.in']
        + [f'tests/scripts/{s}' for s in MAKE_SCRIPTS]},
    'puppet-parser-specs': {
        'kind': 'raw', 'repo': 'puppetlabs/puppet', 'ref': 'main', 'revision': 'e227c27540975c25aa22d533a52424a9d2fc886a',
        'files': ['LICENSE', 'spec/unit/pops/parser/parser_rspec_helper.rb']
        + [f'spec/unit/pops/parser/{s}' for s in PUPPET_SPECS],
        'compare_ref': '8.10.0'},
}


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(repo, revision, path):
    """Return bytes, or None on a 404. Any other failure aborts the acquisition."""
    url = f'{RAW}/{repo}/{revision}/{quote(path)}'
    for attempt in range(8):
        result = subprocess.run(['curl', '-sS', '-L', '--max-time', '60', '-w', '\n%{http_code}', url],
                                capture_output=True)
        if result.returncode:
            raise RuntimeError(f'fetch failed {url}: {result.stderr.decode(errors="replace")}')
        body, _, code = result.stdout.rpartition(b'\n')
        if code != b'429':
            break
        time.sleep(2 ** attempt)
    if code == b'404':
        return None
    if code != b'200':
        raise RuntimeError(f'fetch {url}: HTTP {code.decode()}')
    return body


def write_suite(suite_id, files, manifest):
    target = ROOT / suite_id
    for rel, data in files.items():
        path = target / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    manifest['files'] = {rel: {'bytes': len(data), 'sha256': sha(data), 'url': manifest['url_template'].format(path=rel)}
                         for rel, data in sorted(files.items())}
    (target / 'ACQUISITION.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    print(f'{suite_id}: {len(files)} files')


def acquire_raw(suite_id, meta):
    repo, rev = meta['repo'], meta['revision']
    wanted = list(meta['files'])
    notes = [f"ref {meta['ref']} resolved to {rev} via proxy.golang.org Origin.Hash (2026-11-17)",
             f'every byte fetched from {RAW}/{repo}/{rev}/<path>; a nonexistent commit or path returns 404']
    optional = []
    enumeration = {'method': 'fixed file list', 'complete': None}
    if 'local_index' in meta:
        goroot, rel = meta['local_index']
        names = sorted(p.name for p in (Path(goroot) / rel).iterdir() if p.is_file())
        wanted += [f'{rel}/{n}' for n in names]
        enumeration = {'method': f'file names of {goroot}/{rel} (local go1.24.7 installation); every file re-fetched '
                                 'at the pinned commit and compared byte-for-byte with the local copy',
                       'complete': True, 'listed': len(names)}
    if 'runcmake_dir' in meta:
        index = fetch(repo, rev, meta['runcmake_dir'] + '/RunCMakeTest.cmake').decode()
        names = sorted(set(re.findall(r'^run_cmake\(([A-Za-z0-9_.+-]+)\)', index, re.M)))
        for n in names:
            wanted.append(f"{meta['runcmake_dir']}/{n}.cmake")
            optional += [f"{meta['runcmake_dir']}/{n}-{s}" for s in
                         ('stderr.txt', 'stdout.txt', 'result.txt', 'check.cmake')]
        enumeration = {'method': 'run_cmake(<name>) calls in RunCMakeTest.cmake at the pinned commit; per test '
                                 '<name>.cmake plus any of -stderr.txt/-stdout.txt/-result.txt/-check.cmake that exist',
                       'complete': True, 'listed': len(names),
                       'not_imported': 'run_cmake_command/run_override and include()d helper tests (not run_cmake)'}
    with ThreadPoolExecutor(12) as pool:
        got = dict(zip(wanted + optional, pool.map(lambda p: fetch(repo, rev, p), wanted + optional)))
    files = {}
    for path in wanted:
        if got[path] is None:
            raise RuntimeError(f'{suite_id}: required file missing at pin: {path}')
        files[path] = got[path]
    files.update({p: d for p in optional if (d := got[p]) is not None})
    if 'local_index' in meta:
        goroot, _ = meta['local_index']
        diff = [p for p in files if p.startswith(meta['local_index'][1]) and (Path(goroot) / p).read_bytes() != files[p]]
        if diff:
            raise RuntimeError(f'{suite_id}: pinned bytes differ from the local toolchain: {diff[:5]}')
        notes.append(f'all {enumeration["listed"]} testdata/mod files equal the local {goroot} copy byte-for-byte')
    if 'compare_ref' in meta:
        same, differ = [], []
        for path, data in sorted(files.items()):
            other = fetch(repo, meta['compare_ref'], path)
            (same if other == data else differ).append(path)
        notes.append(f"compared with tag {meta['compare_ref']} (the released gem used as oracle): {len(same)} files "
                     f"identical, differing: {differ or 'none'}; the tag itself could not be resolved to a commit "
                     '(proxy.golang.org returned not found), so the pin is the main commit above')
    write_suite(suite_id, files, {'suite_id': suite_id, 'method': 'raw', 'repo': repo, 'ref': meta['ref'],
                                  'revision': rev, 'enumeration': enumeration, 'verification': notes,
                                  'url_template': f'{RAW}/{repo}/{rev}/{{path}}'})


def acquire_gomod(suite_id, meta):
    env = dict(os.environ)
    out = subprocess.run(['go', 'mod', 'download', '-json', f"{meta['module']}@{meta['version']}"],
                         capture_output=True, check=True, env=env)
    info = json.loads(out.stdout)
    origin = info.get('Origin') or {}
    if origin.get('Hash') != meta['revision']:
        raise RuntimeError(f'{suite_id}: module origin {origin.get("Hash")} != pinned {meta["revision"]}')
    zdata = Path(info['Zip']).read_bytes()
    prefix = f"{meta['module']}@{meta['version']}/"
    files = {}
    with zipfile.ZipFile(info['Zip']) as archive:
        for name in archive.namelist():
            rel = name[len(prefix):]
            if not name.endswith('/') and meta['select'](rel):
                files[rel] = archive.read(name)
    write_suite(suite_id, files, {
        'suite_id': suite_id, 'method': 'gomod', 'module': meta['module'], 'version': meta['version'],
        'revision': meta['revision'], 'module_sum': info['Sum'], 'zip_sha256': sha(zdata), 'zip_bytes': len(zdata),
        'origin': origin, 'enumeration': {'method': 'module zip file list', 'complete': True},
        'verification': ['go mod download verified the zip against sum.golang.org (GONOSUMDB/GOPRIVATE unset)',
                         f"module VCS origin {origin.get('URL')} {origin.get('Ref')} = {origin.get('Hash')}",
                         'files extracted verbatim from the verified zip'],
        'url_template': f"https://proxy.golang.org/{meta['module']}/@v/{meta['version']}.zip#{{path}}"})


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--suite', nargs='*', choices=sorted(SUITES))
    args = parser.parse_args()
    for suite_id in args.suite or sorted(SUITES):
        meta = SUITES[suite_id]
        (acquire_raw if meta['kind'] == 'raw' else acquire_gomod)(suite_id, meta)
    return 0


if __name__ == '__main__':
    sys.exit(main())
