#!/usr/bin/env python3
"""Acquire the #265 structured/data/schema external suites at immutable revisions.

Benchmark tooling only (core #265 / #268). Every byte comes from
``raw.githubusercontent.com/<owner>/<repo>/<40-hex commit>/<path>``: a fetch at an
immutable commit, never a branch. Each suite writes its files under
``tests/fixtures/external/upstream/structured/<suite_id>/`` with an ``ACQUISITION.json``
manifest (URL, SHA-256 and size per file, plus what could and could not be verified).

Directory listings of GitHub trees (github.com HTML, the REST API, git clones) are not
reachable from the benchmark sandbox, so each suite is enumerated from an index file
committed in the suite itself at the same revision (toml-test ``files-toml-1.0.0``,
JSONTestSuite ``results/parsing.html``, buf ``breaking_test.go``, inih ``unittest.c``).
Where no in-suite index exists (YAML Test Suite ``data`` branch) a candidate ID list
is probed and only IDs that exist at the pinned revision are kept; the enumeration is
then recorded as incomplete. The W3C XML suite is hosted on www.w3.org, which is not
reachable: it is recorded as BLOCKED and no other source is substituted.

    python scripts/acquire_structured_benchmarks.py [--suite ID ...]   # network
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures' / 'external' / 'upstream' / 'structured'
RAW = 'https://raw.githubusercontent.com'

# Immutable revisions. Tag/branch -> commit resolution was done with the Go module
# proxy's VCS origin record (`go list -m -json <repo>@<ref>`, Origin.Hash); every file
# is then fetched from raw.githubusercontent.com at that commit, which 404s for a
# commit that does not exist in the repository.
SUITES = {
    'yaml-test-suite': {'repo': 'yaml/yaml-test-suite', 'ref': 'data',
                        'revision': '6ad3d2c62885d82fc349026c136ef560838fdf3d',
                        'licence_repo_revision': 'da267a5c4782e7361e82889e76c0dc7df0e1e870'},
    'jsontestsuite': {'repo': 'nst/JSONTestSuite', 'ref': 'master',
                      'revision': '1ef36fa01286573e846ac449e8683f8833c5b26a'},
    'json-patch-tests': {'repo': 'json-patch/json-patch-tests', 'ref': 'master',
                         'revision': '2a928f9044aad35c74e2788d498bcf2c6b91adea'},
    'toml-test': {'repo': 'toml-lang/toml-test', 'ref': 'v1.6.0',
                  'revision': '369cc3ae275fd76b6e21780ebf87609e5b2a951a'},
    'cpython-configparser': {'repo': 'python/cpython', 'ref': 'v3.13.18',
                             'revision': '0d4bd512fc429e3d4eacdd113c975e161f21644d'},
    'inih': {'repo': 'benhoyt/inih', 'ref': 'master',
             'revision': '2bbdec4a366c8c39746ee0982e7ca0febbb044b6'},
    'buf-breaking': {'repo': 'bufbuild/buf', 'ref': 'v1.75.0',
                     'revision': '1a3a05a3e0fa350c4b814b7f48aa0e64e9a906bd'},
    'graphql-js-breaking': {'repo': 'graphql/graphql-js', 'ref': 'v16.15.0',
                            'revision': '2b4f68feaed36fd3b221764a467c6fa78a0c9797'},
}

# YAML Test Suite: the `data` branch has no index file. These IDs are a candidate list
# recalled from prior knowledge of the suite; only those that exist at the pinned
# revision are kept, so the list can only under-enumerate, never invent cases.
YAML_CANDIDATES = '''
229Q 236B 26DV 27NA 2AUY 2CMS 2EBW 2G84 2JQS 2LFX 2SXE 2XXW 33X3 35KP 36F6 3ALJ 3GZX 3HFZ
3MYT 3R3P 3RLN 3UYS 4ABK 4CQQ 4EJS 4FJ6 4GC6 4H7K 4HVU 4JVG 4MUZ 4Q9F 4QFQ 4RWC 4UYU 4V8U
4WA9 4ZYM 52DL 54T7 55WF 565N 57H4 58MP 5BVJ 5C5M 5GBF 5KJE 5LLU 5MUD 5NYZ 5T43 5TRB 5TYM
5U3A 5WE3 62EZ 6BCT 6BFJ 6CA3 6CK3 6FWR 6H3V 6HB6 6JQW 6JTT 6JWB 6KGN 6LVF 6M2F 6PBE 6S55
6SLA 6VJK 6WLZ 6WPF 6XDY 6ZKB 735Y 74H7 753E 7A4E 7BMT 7BUB 7FWL 7LBH 7MNF 7QN8 7T8X 7TMG
7W2P 7Z25 7ZZ5 82AN 87E4 8CWC 8G76 8KB6 8MK2 8QBE 8UDB 8XDJ 8XYN 93JH 93WF 96L6 96NN 98YD
9BXH 9C9N 9CWY 9DXL 9FMG 9HCY 9JBA 9KAX 9KBC 9MAG 9MMA 9MMW 9MQT 9SA2 9SHH 9TFX 9U5K 9WXW
9YRD A2M4 A6F9 A984 AB8U AVM7 AZ63 AZW3 B3HG B63P BD7L BEC7 BF9H BS4K BU8L C2DT C2SP C4HZ
CC74 CN3R CPZ3 CQ3W CT4Q CTN5 CUP7 CVW2 CXX2 D49Q D88J D9TU DBG4 DC7X DE56 DFF7 DHP8 DK3J
DK4H DK95 DMG6 DWX9 E76Z EB22 EHF6 EW3V EX5H EXG3 F2C7 F3CP F6MC F8F9 FBC9 FH7J FP8R FQ7F
FRK4 FTA2 FUP4 G4RS G5U8 G7JE G992 G9HC GDY7 GH63 GT5M H2RW H3Z8 H7J7 H7TQ HM87 HMK4 HMQ5
HRE5 HS5T HU3P HWV9 J3BT J5UC J7PZ J7VC J9HZ JEF9 JHB9 JKF3 JQ4R JR7V JS2J JTV5 JY7Z K3WX
K4SU K527 K54U K858 KH5V KK5P KMK3 KS4U KSS4 L24T L383 L94M L9U5 LE5A LHL4 LP6E LQZ7 LX3P
M29M M2N8 M5C3 M5DY M6YH M7A3 M7NX M9B4 MJS9 MUS6 MXS3 MYW6 MZX3 N4JP N782 NAT4 NB6Z NHX8
NJ66 NKF9 NP9H P2AD P2EQ P76L P94K PBJ2 PRH3 PUW8 PW8X Q4CL Q5MG Q88A Q8AD Q9WF QB6E QF4Y
QLJ7 QT73 R4YG R52L RHX7 RLU9 RR7F RTP8 RXY3 RZP5 RZT7 S3PD S4GJ S4JQ S4T7 S7BG S98Z S9E8
SBG9 SF5V SKE5 SM9W SR86 SSW6 SU5Z SU74 SY6V SYW4 T26H T4YY T5N4 T833 TD5N TE2A TL85 TS54
U3C3 U3XV U44R U99R U9NS UDM2 UDR7 UGM3 UKK6 UT92 UV7Q V55R V9D5 VJP3 W42U W4TN W5VH W9L4
WZ62 X38W X4QW X8DW XLQ9 XV9V XW4D Y2GN Y79Y YD5X YJV2 Z67P Z9M4 ZCZ6 ZF4X ZH7C ZK9H ZL4Z
ZVH3 ZWK4 ZXT5 ZYU8
'''.split()
YAML_FILES = ('===', 'in.yaml', 'in.json', 'out.yaml', 'error', 'test.event')


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
        time.sleep(2 ** attempt)  # rate limited: back off, then retry the same immutable URL
    if code == b'404':
        return None
    if code != b'200':
        raise RuntimeError(f'fetch {url}: HTTP {code.decode()}')
    return body


class Acquirer:
    def __init__(self, suite_id):
        self.suite_id = suite_id; self.meta = SUITES[suite_id]
        self.dir = ROOT / suite_id; self.files = {}; self.absent = []; self.notes = []

    def get(self, path, *, required=True, repo=None, revision=None, retain=True):
        repo = repo or self.meta['repo']; revision = revision or self.meta['revision']
        data = fetch(repo, revision, path)
        local = path if repo == self.meta['repo'] and revision == self.meta['revision'] \
            else f'_{repo.replace("/", "__")}@{revision[:12]}/{path}'
        if data is None:
            if required:
                raise RuntimeError(f'{self.suite_id}: required file missing at pin: {path}')
            self.absent.append(path)
            return None
        record = {'url': f'{RAW}/{repo}/{revision}/{path}', 'sha256': hashlib.sha256(data).hexdigest(),
                  'bytes': len(data)}
        if retain:
            target = self.dir / local
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            self.files[local] = record
        else:  # index-only input: hash recorded, bytes not vendored
            self.indexes = getattr(self, 'indexes', {}); self.indexes[local] = record
        return data

    def many(self, paths, required=False):
        with ThreadPoolExecutor(6) as pool:
            return dict(zip(paths, pool.map(lambda p: self.get(p, required=required), paths)))

    def finish(self, enumeration, **extra):
        manifest = {'schema_version': 1, 'suite_id': self.suite_id, 'repository': self.meta['repo'],
                    'ref': self.meta['ref'], 'revision': self.meta['revision'],
                    'revision_resolution': f"Go module proxy VCS origin for {self.meta['repo']}@{self.meta['ref']} "
                                           '(Origin.Hash); bytes fetched only at that commit',
                    'enumeration': enumeration, 'files': dict(sorted(self.files.items())),
                    'probed_absent': sorted(self.absent), 'notes': self.notes,
                    'index_files_not_retained': dict(sorted(getattr(self, 'indexes', {}).items())), **extra}
        (self.dir / 'ACQUISITION.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
        print(f'{self.suite_id}: {len(self.files)} files, {len(self.absent)} probed-absent')


def acquire_yaml():
    a = Acquirer('yaml-test-suite')
    a.get('License', repo=a.meta['repo'], revision=a.meta['licence_repo_revision'])
    found = []
    tops = a.many([f'{i}/===' for i in YAML_CANDIDATES])
    ids = []
    for i in YAML_CANDIDATES:
        if tops[f'{i}/==='] is not None:
            ids.append(i)
        else:
            subs = a.many([f'{i}/{n:02}/===' for n in range(10)])
            ids += [f'{i}/{n:02}' for n in range(10) if subs[f'{i}/{n:02}/==='] is not None]
    for case in ids:
        a.many([f'{case}/{f}' for f in YAML_FILES if f != '==='])
        found.append(case)
    a.finish({'method': 'probe a recalled candidate ID list (no in-suite index on the data branch)',
              'complete': False, 'candidates': len(YAML_CANDIDATES), 'found': found})


def acquire_jsontestsuite():
    a = Acquirer('jsontestsuite')
    a.get('LICENSE'); index = a.get('results/parsing.html', retain=False)  # 1.4 MB generated report
    names = sorted(set(re.findall(rb'\b([yni]_[A-Za-z0-9_.+#-]+\.json)', index)))
    names = [n.decode() for n in names]
    got = a.many([f'test_parsing/{n}' for n in names])
    missing = [n for n in names if got[f'test_parsing/{n}'] is None]
    a.notes.append('Enumeration from the in-repository results/parsing.html at the pinned commit; '
                   'test_parsing files absent from that generated index are not discovered (lower bound).')
    a.finish({'method': 'names listed in results/parsing.html', 'complete': False, 'listed': names,
              'listed_but_missing': missing})


def acquire_json_patch():
    a = Acquirer('json-patch-tests')
    for p in ('README.md', 'tests.json', 'spec_tests.json'):
        a.get(p)
    a.notes.append('Licence: Apache-2.0, declared in README.md "License" section (no LICENSE file at this commit).')
    a.finish({'method': 'fixed file names documented in README.md', 'complete': True})


def acquire_toml():
    a = Acquirer('toml-test')
    a.get('COPYING'); a.get('README.md')
    listing = a.get('tests/files-toml-1.0.0').decode().split()
    a.many([f'tests/{p}' for p in listing], required=True)
    a.finish({'method': 'tests/files-toml-1.0.0 (the suite\'s own TOML 1.0.0 manifest)', 'complete': True,
              'listed': len(listing)})


def acquire_cpython():
    a = Acquirer('cpython-configparser')
    a.get('LICENSE'); test = a.get('Lib/test/test_configparser.py')
    names = sorted(set(re.findall(rb"cfgparser\.\d+", test)))
    for n in names:
        a.get(f'Lib/test/configdata/{n.decode()}')
    a.finish({'method': 'configdata files referenced by Lib/test/test_configparser.py', 'complete': True,
              'listed': [n.decode() for n in names]})


def acquire_inih():
    a = Acquirer('inih')
    a.get('LICENSE.txt'); a.get('ini.c'); a.get('ini.h')
    unittest = a.get('tests/unittest.c')
    names = sorted(set(re.findall(rb'"([A-Za-z0-9_]+\.ini)"', unittest)))
    for n in names:  # unittest.c deliberately names a missing file (no_file.ini)
        a.get(f'tests/{n.decode()}', required=False)
    for baseline in ('baseline_multi.txt', 'baseline_single.txt', 'baseline_disallow_inline_comments.txt',
                     'baseline_stop_on_first_error.txt', 'baseline_handler_lineno.txt', 'unittest.sh'):
        a.get(f'tests/{baseline}', required=False)
    a.finish({'method': '.ini files named in tests/unittest.c', 'complete': True,
              'listed': [n.decode() for n in names]})


def acquire_buf():
    a = Acquirer('buf-breaking')
    a.get('LICENSE')
    test = a.get('private/bufpkg/bufcheck/breaking_test.go').decode()
    dirs = sorted(set(re.findall(r'testBreaking\(\s*t,\s*"([^"]+)"', test)))
    base = 'private/bufpkg/bufcheck/testdata/breaking'
    # No tree listing: probe the conventional names. A case whose probed files do not
    # reproduce the upstream assertion is rejected by the witness, never patched.
    probes = []
    for d in dirs:
        for side in ('previous', 'current'):
            probes += [f'{base}/{side}/{d}/{n}' for n in ('buf.yaml', 'buf.work.yaml', 'a.proto', 'b.proto')
                       + tuple(f'{n}.proto' for n in range(1, 10))]
    a.many(probes)
    a.notes.append('Per-case directories are not listable; files were probed by conventional names. '
                   'Files under other names or subdirectories are not acquired.')
    a.finish({'method': 'testBreaking case directories named in breaking_test.go; files probed', 'complete': False,
              'listed': dirs})


def acquire_graphql():
    a = Acquirer('graphql-js-breaking')
    a.get('LICENSE'); a.get('package.json')
    a.get('src/utilities/__tests__/findBreakingChanges-test.ts')
    a.finish({'method': 'single upstream test module', 'complete': True})


ACQUIRE = {'yaml-test-suite': acquire_yaml, 'jsontestsuite': acquire_jsontestsuite,
           'json-patch-tests': acquire_json_patch, 'toml-test': acquire_toml,
           'cpython-configparser': acquire_cpython, 'inih': acquire_inih, 'buf-breaking': acquire_buf,
           'graphql-js-breaking': acquire_graphql}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--suite', nargs='+', choices=sorted(ACQUIRE))
    args = parser.parse_args(argv)
    for suite in args.suite or sorted(ACQUIRE):
        ACQUIRE[suite]()
    return 0


if __name__ == '__main__':
    sys.exit(main())
