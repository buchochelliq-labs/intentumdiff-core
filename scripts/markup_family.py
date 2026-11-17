#!/usr/bin/env python3
"""#263 web, markup and embedded-language family for the schema-2 admission ledger (core #268).

Benchmark tooling only. Covers the registered IDs markdown, mdx, html, css, scss, vue,
svelte, astro, latex, asciidoc, po, postscript and generic. Inputs are the revision-pinned
upstream bytes acquired by ``scripts/acquire_markup_benchmarks.py`` under
``tests/fixtures/external/upstream/markup/``. This is a fragment family (the #265
mechanism): the fragment ``tests/fixtures/external/admission/families/markup-263.json`` is
written and ``--check``-ed here and merged verbatim by ``scripts/build_external_admission.py``.

Two lanes are kept apart:

* ``parser_fidelity``: one upstream document on both sides (old == new), with
  ``expected.parse.accepts`` from the upstream label confirmed by an executed reference
  (CommonMark and GFM expected HTML reproduced by commonmark.js / cmark-gfm, the WPT
  html5lib tree reproduced by html5lib, css-parsing-tests ASTs reproduced by tinycss2,
  sass-spec CSS reproduced by Dart Sass, Svelte / Vue / Astro / MDX compilers,
  Asciidoctor, GNU msgfmt and pdfTeX). For CommonMark examples the reference block
  inventory (commonmark.js ``sourcepos``) is also asserted (``statement_inventory``).
* ``change_understanding``: Svelte's own migrate input/output pairs (``upstream_gold``) and
  ``reviewed_external_derived`` edits of upstream seeds. Each changed/preserved claim is
  established by a reference renderer or compiler run outside IntentumDiff (rendered HTML,
  html5lib tree, tinycss2 AST, compiled CSS, server-rendered HTML, compiled module, .mo
  bytes or DVI bytes), never by IntentumDiff output.

Commands::

    python scripts/markup_family.py witness [--suite S ...]   # run reference oracles
    python scripts/markup_family.py build [--check]           # (re)write the fragment

Witness execution needs the reference toolchains named in ``TOOLS``; ``build`` is
offline and deterministic.
"""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures' / 'external'
UP = 'upstream/markup'
DERIVED = 'admission/families/markup-263'
WITNESS_DIR = 'admission/witnesses/markup'
FRAGMENT = ROOT / 'admission' / 'families' / 'markup-263.json'
_spec = importlib.util.spec_from_file_location('external_ledger', Path(__file__).with_name('external_ledger.py'))
L = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(L)
_spec = importlib.util.spec_from_file_location('structured_family', Path(__file__).with_name('structured_family.py'))
S = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(S)
sha, encode = L.sha, L.encode
# Shared adapters (one schema for every family): expectations and byte deltas.
minimal_region, classification, forbid, NO_FALLBACK = S.minimal_region, S.classification, S.forbid, S.NO_FALLBACK
expect_accept, expect_reject, expect_changed = S.expect_accept, S.expect_reject, S.expect_changed

# Reference toolchains (witness only). Versions are recorded in every witness record.
TOOLS = {
    'node_modules': os.environ.get('MARKUP_REF_NODE_MODULES', ''),  # commonmark, sass, svelte, vue, @astrojs/compiler, @mdx-js/mdx
    'python': os.environ.get('MARKUP_REF_PYTHON', sys.executable),  # html5lib 1.1, tinycss2, cmarkgfm
    'asciidoctor': os.environ.get('MARKUP_REF_ASCIIDOCTOR', 'asciidoctor'),
    'msgfmt': os.environ.get('MARKUP_REF_MSGFMT', 'msgfmt'),
    'latex': os.environ.get('MARKUP_REF_LATEX', 'latex'),
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


def expect_preserved(forbidden_types, reason):
    return {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
            'forbidden': [forbid('change_type', reason, t) for t in forbidden_types] + [NO_FALLBACK]}


def expect_rewrite(old, new, reason):
    """Equivalent rewrite (e.g. a Svelte 5 migration): behaviour preserved, but the source
    rewrite must be explained structurally; it is neither style-only nor invisible."""
    o, n = minimal_region(old, new)
    return {'classification': classification(None, False, False), 'alternatives': [],
            'required': [{'kind': 'edit_region', 'change_types_any_of': ALL_TYPES, 'old_range': o, 'new_range': n,
                          'evidence': f'{reason}; a reported change must touch the independently computed delta '
                                      f'{o["start_byte"]}..{o["end_byte"]} (old) / {n["start_byte"]}..{n["end_byte"]} (new)'}],
            'forbidden': [forbid('style_only', 'an equivalent API rewrite is not formatting'),
                          forbid('no_changes', 'the rewrite must be reported'), NO_FALLBACK]}


# --- suite metadata ----------------------------------------------------------------------

LIC = {
    'commonmark-spec': ('CC-BY-SA-4.0', 'package/LICENSE in the npm tarball and the spec.txt front matter '
                        '(license: CC-BY-SA 4.0)', 'CommonMark Spec 0.31.2, John MacFarlane (spec.commonmark.org), '
                        'via the commonmark-spec npm package'),
    'gfm-spec': ('CC-BY-SA-4.0', 'test/spec.txt front matter (license: CC-BY-SA 4.0); COPYING covers cmark-gfm code',
                 'GitHub Flavored Markdown Spec 0.29-gfm, John MacFarlane and GitHub, Inc. (github/cmark-gfm)'),
    'wpt-html-tree-construction': ('BSD-3-Clause', 'LICENSE.md at the pinned WPT commit (3-Clause BSD); the .dat files '
                                   'originate in html5lib-tests (MIT)',
                                   'web-platform-tests contributors; html5lib tree-construction tests (James Graham, '
                                   'Geoffrey Sneddon and contributors)'),
    'css-parsing-tests': ('CC0-1.0', 'LICENSE at the pinned commit (public-domain dedication)',
                          'css-parsing-tests, Simon Sapin (github.com/SimonSapin/css-parsing-tests)'),
    'sass-spec': ('MIT', 'package/LICENSE in the npm tarball (gitHead is the pinned commit)',
                  'Sass spec suite, Sass team and contributors (github.com/sass/sass-spec), sass-spec npm 3.5.4-1'),
    'svelte-tests': ('MIT', 'LICENSE.md at the pinned commit', 'Svelte, Svelte contributors (github.com/sveltejs/svelte)'),
    'vue-compiler-sfc-tests': ('MIT', 'LICENSE at the pinned commit', 'Vue.js core, Yuxi (Evan) You and Vue contributors '
                               '(github.com/vuejs/core, packages/compiler-sfc/__tests__)'),
    'astro-compiler-tests': ('MIT', 'LICENSE at the pinned commit', 'Astro compiler, withastro contributors '
                             '(github.com/withastro/compiler, internal/printer/printer_test.go)'),
    'mdx-docs': ('MIT', 'license at the pinned commit', 'MDX, Compositor and Vercel, Inc. and MDX contributors '
                 '(github.com/mdx-js/mdx, docs/)'),
    'asciidoctor-doctest': ('MIT', 'LICENSE at the pinned commit', 'Asciidoctor DocTest, Jakub Jirutka and the Asciidoctor '
                            'Project (data/examples/asciidoc)'),
    'polib-tests': ('MIT', 'LICENSE at the pinned commit', 'polib test fixtures, David Jean Louis (github.com/izimobil/polib)'),
    'latex2e-base': ('LPPL-1.3c', 'LICENSE at the pinned commit (LaTeX Project Public License 1.3c; verbatim '
                     'redistribution permitted; derived files are renamed and documented as modified)',
                     'LaTeX2e, The LaTeX Project (github.com/latex3/latex2e, base/)'),
}
SUITE_INFO = {
    'commonmark-spec': ('CommonMark Spec examples', '0.31.2', 'https://github.com/commonmark/commonmark-spec',
                        'standards / conformance suite', 'markdown'),
    'gfm-spec': ('GitHub Flavored Markdown Spec extension examples', '0.29.0.gfm.13', 'https://github.com/github/cmark-gfm',
                 'standards / conformance suite', 'markdown'),
    'wpt-html-tree-construction': ('WPT html/syntax/parsing tree-construction (html5lib format)', 'master @edd042b6',
                                   'https://github.com/web-platform-tests/wpt/tree/master/html/syntax/parsing',
                                   'standards / conformance suite', 'html'),
    'css-parsing-tests': ('css-parsing-tests (CSS Syntax Level 3)', 'master @203ce36b',
                          'https://github.com/SimonSapin/css-parsing-tests', 'standards / conformance suite', 'css'),
    'sass-spec': ('Sass spec suite', 'npm sass-spec 3.5.4-1 (gitHead 8be4e1b7)', 'https://github.com/sass/sass-spec',
                  'official upstream regression suite', 'scss'),
    'svelte-tests': ('Svelte parser and migrate test samples', 'svelte@5.57.3',
                     'https://github.com/sveltejs/svelte/tree/main/packages/svelte/tests', 'official upstream regression suite',
                     'svelte'),
    'vue-compiler-sfc-tests': ('Vue compiler-sfc regression tests', 'v3.5.45',
                               'https://github.com/vuejs/core/tree/main/packages/compiler-sfc/__tests__',
                               'official upstream regression suite', 'vue'),
    'astro-compiler-tests': ('Astro compiler printer regression fixtures', '@astrojs/compiler@4.0.0',
                             'https://github.com/withastro/compiler/blob/main/internal/printer/printer_test.go',
                             'official upstream regression suite', 'astro'),
    'mdx-docs': ('MDX project documentation sources', '3.1.1', 'https://github.com/mdx-js/mdx/tree/main/docs',
                 'qualified external candidate; project sample corpus', 'mdx'),
    'asciidoctor-doctest': ('Asciidoctor DocTest reference examples', 'master @1f48884c',
                            'https://github.com/asciidoctor/asciidoctor-doctest', 'established community / compatibility suite',
                            'asciidoc'),
    'polib-tests': ('polib PO test fixtures', 'master @bcb2bbab', 'https://github.com/izimobil/polib',
                    'established community / compatibility suite', 'po'),
    'latex2e-base': ('LaTeX2e distribution documents (base/manifest.txt)', 'develop @bd4c0b10',
                     'https://github.com/latex3/latex2e', 'official upstream sample corpus', 'latex'),
}
DIALECT = {
    'markdown': 'CommonMark 0.31.2 (reference: commonmark.js 0.31.2 HtmlRenderer, default options)',
    'gfm': 'GitHub Flavored Markdown 0.29-gfm (reference: cmark-gfm via cmarkgfm, extensions table, strikethrough, '
           'autolink, tagfilter, tasklist; unsafe raw HTML allowed as in the spec runner)',
    'html': 'HTML Living Standard tree construction, scripting disabled (reference: html5lib 1.1, dom builder, namespaceHTMLElements off)',
    'css': 'CSS Syntax Level 3 (reference: tinycss2, comments and whitespace skipped)',
    'scss': 'SCSS (reference: Dart Sass npm sass 1.108.2, expanded style)',
    'svelte': 'Svelte 5 component (reference: svelte 5.57.3 compiler and svelte/server render)',
    'vue': 'Vue 3 SFC (reference: @vue/compiler-sfc 3.5.45 and vue/server-renderer 3.5.45)',
    'astro': 'Astro component (reference: @astrojs/compiler 4.0.0 transform)',
    'mdx': 'MDX 3 (reference: @mdx-js/mdx 3.1.1 compile, default options)',
    'asciidoc': 'AsciiDoc (reference: Asciidoctor 2.0.26 html5 backend, embedded)',
    'po': 'GNU gettext PO (reference: GNU msgfmt 0.21 --check)',
    'latex': 'LaTeX2e (reference: TeX Live 2023 pdfTeX 1.40.25 in DVI mode, SOURCE_DATE_EPOCH fixed)',
}

BLOCKED = [
    {'language_id': 'postscript', 'suite_id': 'ghostscript-examples', 'name': 'Ghostscript public PostScript samples',
     'url': 'https://github.com/ArtifexSoftware/ghostpdl/tree/master/examples',
     'lanes': ['parser_fidelity', 'change_understanding'],
     'reason': 'BLOCKED by licence: the selected Ghostscript examples are distributed under AGPL-3.0 (ghostpdl), which '
               'is not a permissive licence for vendoring; no permissive PostScript suite with an oracle was found, so '
               'nothing was vendored and no other source was substituted.'},
    {'language_id': 'generic', 'suite_id': 'wiki-atomic-edits', 'name': 'WikiAtomicEdits',
     'url': 'https://github.com/google-research-datasets/wiki-atomic-edits',
     'lanes': ['parser_fidelity', 'change_understanding'],
     'reason': 'BLOCKED: the dataset files are only published at storage.googleapis.com/wiki-atomic-edits/..., which '
               'answers HTTP 403 from the benchmark sandbox (2026-11-17); the GitHub repository (README at '
               '1f6769f2f9d93b6bf5acb091eb509b2be36e8e79) holds no data. No other prose corpus was substituted.'},
]


# --- case construction ---------------------------------------------------------------------

def case(*, case_id, suite, language, dialect, filename, lane, family, upstream_case, record, old, new, derivation,
         claim, assumptions, expected, plan, review, fields):
    for ref in (old, new):
        if ref['path'] in PENDING:
            write(ref['path'], PENDING[ref['path']])
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
        if callable(expected):
            expected = expected(record_obj)
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
                   'notes': 'Reference implementation executed outside IntentumDiff by scripts/markup_family.py '
                            'witness; IntentumDiff output is not an input to this oracle.'},
        'admission': {'status': status, 'reason': reason},
        'expected': expected if admitted else None,
        'witness_plan': plan,
    }


PENDING = {}


def extracted(suite, case_id, data: bytes, ext, how):
    """An example extracted byte-exact from an upstream file (verbatim content). Its bytes are
    written only once a case uses them (see ``case``), so unused seeds leave no files."""
    rel = f'{DERIVED}/{suite}/{case_id}.{ext}'
    PENDING[rel] = data
    return {'path': rel, 'sha256': sha(data)}, [{'step': 'verbatim_upstream', 'description': how}]


def parser_case(*, case_id, suite, language, dialect, filename, doc, derivation, record, accepts, plan, upstream_case,
                label, fields, inventory=False):
    if inventory:
        def expected(rec, _accept=accepts):
            out = expect_accept()
            inv = (rec.get('observations') or {}).get('inventory')
            if inv:
                out['parse'] = {'accepts': True, 'statement_inventory': inv}
            return out
    else:
        expected = expect_accept() if accepts else expect_reject()
    return case(case_id=case_id, suite=suite, language=language, dialect=dialect, filename=filename,
                lane='parser_fidelity', family='accept' if accepts else 'reject', upstream_case=upstream_case,
                record=record, old=doc, new=doc, derivation=derivation, claim='preserved',
                assumptions=f'Single-source conformance: upstream {label} says the document must be '
                            f'{"accepted" if accepts else "rejected"}; old and new are the identical upstream bytes.',
                expected=expected, plan={**plan, 'accepts': accepts},
                review=f'Upstream {label} and the executed reference agree.', fields=fields)


def derived_pair(suite, language, dialect, filename, stem, upstream_case, record, old_ref, old_b, new_b, family, claim,
                 description, assumptions, expected, plan, fields):
    ext = filename.rsplit('.', 1)[1]
    new = write(f'{DERIVED}/{suite}/{stem}-new.{ext}', new_b)
    return case(case_id=stem, suite=suite, language=language, dialect=dialect, filename=filename,
                lane='change_understanding', family=family, upstream_case=upstream_case, record=record, old=old_ref,
                new=new, derivation=[{'step': 'derived_transformation', 'description': description,
                                      'tool': 'scripts/markup_family.py'}],
                claim=claim, assumptions=assumptions, expected=expected, plan=plan,
                review=f'Derivation reviewed: {description}.', fields=fields)


class Deriver:
    """Bounded derivation families over sorted seeds (DERIVED_LIMIT pairs per family)."""

    def __init__(self, suite, language, dialect, filename, oracle):
        self.suite, self.language, self.dialect, self.filename, self.oracle = suite, language, dialect, filename, oracle
        self.counts = Counter(); self.out = []

    def add(self, family, seed, transform, claim, description, assumptions, expected_fn):
        """seed = (seed_id, upstream_case, record_ref, old_ref, old_bytes). transform -> new bytes or None."""
        if self.counts[family] >= DERIVED_LIMIT:
            return
        seed_id, upstream_case, record, old_ref, old_b = seed
        new_b = transform(old_b)
        if new_b is None or new_b == old_b or not utf8(new_b):
            return
        self.counts[family] += 1
        stem = f'{seed_id}-{family}'
        self.out.append(derived_pair(self.suite, self.language, self.dialect, self.filename, stem, upstream_case, record,
                                     old_ref, old_b, new_b, family, claim, description, assumptions,
                                     expected_fn(old_b, new_b), {'oracle': self.oracle}, [record['path']]))


def sub_first(pattern, repl, flags=0):
    rx = re.compile(pattern, flags)

    def apply(data):
        m = rx.search(data)
        if not m:
            return None
        return data[:m.start()] + (repl(m) if callable(repl) else m.expand(repl)) + data[m.end():]
    return apply


def changed(types, evidence):
    return lambda o, n: expect_changed(o, n, types, evidence)


def preserved(types, reason):
    return lambda o, n: expect_preserved(types, reason)


def discovery(suite, language, lane, discovered, eligible, rule, holdout='none'):
    return {'suite_id': suite, 'language_id': language, 'lane': lane, 'discovered': discovered, 'eligible': eligible,
            'eligibility_rule': rule, 'imported_rows': 'all eligible', 'holdout': holdout}


# Markdown (CommonMark, GFM) ----------------------------------------------------------------

FENCE = '`' * 32


def spec_examples(text):
    """The spec runner's own extraction: markdown / '.' / html between example fences; '→' is a tab."""
    out = []; state = 0; section = None; md = []; html = []; tag = ''; number = 0
    for line in text.splitlines(keepends=True):
        stripped = line.rstrip('\n')
        if state == 0 and stripped.startswith(FENCE + ' example'):
            state, md, html, tag = 1, [], [], stripped[len(FENCE) + len(' example'):].strip()
        elif state == 1 and stripped == '.':
            state = 2
        elif state == 2 and stripped == FENCE:
            number += 1
            out.append({'number': number, 'section': section, 'tag': tag,
                        'markdown': ''.join(md).replace('→', '\t'), 'html': ''.join(html).replace('→', '\t')})
            state = 0
        elif state == 1:
            md.append(line)
        elif state == 2:
            html.append(line)
        elif state == 0:
            m = re.match(r'#{1,6} +(.*)', stripped)
            if m:
                section = m.group(1).strip()
    return out


def markdown_cases():
    out = []; disc = []
    spec = upbytes('commonmark-spec', 'package/spec.txt').decode('utf-8')
    record = upref('commonmark-spec', 'package/spec.txt')
    examples = spec_examples(spec)
    seeds = []
    for ex in examples:
        cid = f"cm-{ex['number']:04}"
        data = ex['markdown'].encode('utf-8')
        doc, deriv = extracted('commonmark-spec', cid, data, 'md',
                               f"example {ex['number']} extracted byte-exact from spec.txt by the spec's own rule "
                               '(markdown before the "." line; → is a tab)')
        out.append(parser_case(case_id=cid, suite='commonmark-spec', language='markdown', dialect=DIALECT['markdown'],
                               filename='fixture.md', doc=doc, derivation=deriv, record=record, accepts=True,
                               plan={'oracle': 'commonmark_example', 'example': ex['number']},
                               upstream_case=f"example {ex['number']} ({ex['section']})",
                               label='CommonMark example (every input is a valid document; expected HTML given)',
                               fields=['markdown', 'html'], inventory=True))
        seeds.append((cid, f"example {ex['number']}", record, doc, data))
    disc.append(discovery('commonmark-spec', 'markdown', 'parser_fidelity', len(examples), len(examples),
                          'every spec example (complete enumeration); must accept, block inventory from commonmark.js '
                          'sourcepos where every non-blank line belongs to a top-level block'))
    d = Deriver('commonmark-spec', 'markdown', DIALECT['markdown'], 'fixture.md', 'commonmark_render')
    A = 'CommonMark 0.31.2 rendering with commonmark.js defaults; equivalence is equal rendered HTML.'
    for seed in seeds:
        d.add('emphasis_delimiter', seed, sub_first(rb'(?<![*_\\\w])\*([A-Za-z]+)\*(?![*_\w])', rb'_\1_'), 'preserved',
              'first `*word*` emphasis respelled `_word_`', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'the same emphasis element renders'))
        d.add('link_destination', seed, sub_first(rb'\]\((/[A-Za-z0-9_/.-]*[A-Za-z0-9])([ )])', rb'](\1x\2'), 'changed',
              'first inline link destination gains one character', A,
              changed(EDIT_TYPES, 'the rendered href changes'))
        d.add('heading_level', seed, sub_first(rb'(?m)^# (?=\S)', b'## '), 'changed',
              'first ATX level-1 heading becomes level 2', A, changed(EDIT_TYPES, 'h1 becomes h2'))
        d.add('word_edit', seed, sub_first(rb'(?m)\A([A-Za-z]{3,})(?=[ \n])', rb'\1x'), 'changed',
              'first word of the document gains one letter', A, changed(EDIT_TYPES, 'rendered text changes'))
        d.add('trailing_blank_line', seed, lambda b: b + b'\n' if b.endswith(b'\n') else None, 'preserved',
              'one blank line appended at the end of the document', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'a trailing blank line renders nothing'))
        d.add('bullet_marker', seed, lambda b: re.sub(rb'(?m)^- (?=[A-Za-z])', b'* ', b) if re.search(rb'(?m)^- [A-Za-z]', b)
              and not re.search(rb'(?m)^\* ', b) else None, 'preserved',
              'every top-level `- ` bullet marker respelled `* `', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'the same list renders'))
    out += d.out
    disc.append(discovery('commonmark-spec', 'markdown', 'change_understanding', 0, 0,
                          'the spec supplies no before/after pairs; only reviewed_external_derived edits of its examples '
                          '(counted on their own rows)'))
    # GFM: only the extension examples (the others duplicate CommonMark examples).
    gspec = upbytes('gfm-spec', 'test/spec.txt').decode('utf-8')
    grec = upref('gfm-spec', 'test/spec.txt')
    gex = spec_examples(gspec)
    ext = [e for e in gex if e['tag']]
    for ex in ext:
        cid = f"gfm-{ex['number']:04}"
        doc, deriv = extracted('gfm-spec', cid, ex['markdown'].encode('utf-8'), 'md',
                               f"extension example {ex['number']} ({ex['tag']}) extracted byte-exact from test/spec.txt "
                               "by the spec's own rule (→ is a tab)")
        out.append(parser_case(case_id=cid, suite='gfm-spec', language='markdown', dialect=DIALECT['gfm'],
                               filename='fixture.md', doc=doc, derivation=deriv, record=grec, accepts=True,
                               plan={'oracle': 'gfm_example', 'example': ex['number'], 'extension': ex['tag']},
                               upstream_case=f"example {ex['number']} ({ex['tag']}, {ex['section']})",
                               label='GFM extension example (valid input; expected HTML given)',
                               fields=['markdown', 'html']))
    disc.append(discovery('gfm-spec', 'markdown', 'parser_fidelity', len(gex), len(ext),
                          'examples tagged with a GFM extension; the untagged examples duplicate CommonMark (0.29) '
                          'examples and are deduplicated against commonmark-spec'))
    return out, disc


# MDX -----------------------------------------------------------------------------------------

def mdx_cases():
    out = []; disc = []
    acq = acquisition('mdx-docs')
    paths = [p for p in acq['enumeration']['found'] if p.endswith('.mdx')]
    seeds = []
    for p in paths:
        cid = 'mdx-' + p.removeprefix('docs/').removesuffix('.mdx').replace('/', '-')
        ref = upref('mdx-docs', p)
        out.append(parser_case(case_id=cid, suite='mdx-docs', language='mdx', dialect=DIALECT['mdx'],
                               filename='fixture.mdx', doc=ref, derivation=S.VERBATIM, record=ref, accepts=True,
                               plan={'oracle': 'mdx_compile'}, upstream_case=p,
                               label='published documentation source (compiled by the project site build)', fields=[p]))
        seeds.append((cid, p, ref, ref, upbytes('mdx-docs', p)))
    disc.append(discovery('mdx-docs', 'mdx', 'parser_fidelity', len(paths), len(paths),
                          '.mdx files under docs/ at the pinned tag. Enumeration INCOMPLETE: probed recalled paths; the '
                          'mdx-js compile.js test strings were not extracted'))
    d = Deriver('mdx-docs', 'mdx', DIALECT['mdx'], 'fixture.mdx', 'mdx_compile_equal')
    A = 'MDX 3 compile with default options; equivalence is an identical compiled module.'
    for seed in seeds:
        d.add('word_edit', seed, sub_first(rb'(?m)^([A-Z][a-z]{2,})(?= [a-z])', rb'\1x'), 'changed',
              'first capitalised word that starts a prose line gains one letter', A,
              changed(EDIT_TYPES, 'rendered text changes'))
        d.add('emphasis_delimiter', seed, sub_first(rb'(?<![*_\\\w`])\*([A-Za-z]+)\*(?![*_\w`])', rb'_\1_'), 'preserved',
              'first `*word*` emphasis respelled `_word_`', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'the same emphasis element is compiled'))
        d.add('heading_level', seed, sub_first(rb'(?m)^## (?=[A-Za-z])', b'### '), 'changed',
              'first level-2 heading becomes level 3', A, changed(EDIT_TYPES, 'h2 becomes h3'))
    out += d.out
    disc.append(discovery('mdx-docs', 'mdx', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits (own rows)'))
    return out, disc


# HTML (WPT tree construction) ------------------------------------------------------------------

DAT_HEADERS = ('#data', '#errors', '#new-errors', '#document-fragment', '#script-off', '#script-on', '#document')


def dat_tests(text):
    """html5lib .dat reader: sections start at a known '#' header; #data loses its final newline."""
    tests = []; cur = None; section = None
    for line in text.split('\n'):
        if line == '#data':
            if cur:
                tests.append(cur)
            cur = {'#data': []}; section = '#data'
        elif cur is not None and line in DAT_HEADERS:
            section = line; cur[section] = []
        elif cur is not None:
            cur[section].append(line)
    if cur:
        tests.append(cur)
    out = []
    for t in tests:
        doc = t.get('#document', [])
        while doc and doc[-1] == '':
            doc.pop()
        out.append({'data': '\n'.join(t['#data']), 'errors': [e for e in t.get('#errors', []) if e],
                    'new_errors': [e for e in t.get('#new-errors', []) if e],
                    'fragment': '#document-fragment' in t, 'script': 'on' if '#script-on' in t else
                    ('off' if '#script-off' in t else None), 'document': '\n'.join(doc)})
    return out


_ATTR = rb'[a-zA-Z][a-zA-Z0-9-]*(?:="[^"<>]*"|=\'[^\'<>]*\')'


def html_cases():
    out = []; disc = []
    acq = acquisition('wpt-html-tree-construction')
    files = sorted(p for p in acq['enumeration']['found'])
    total = 0; eligible = 0; seeds = []
    for path in files:
        rel = path
        stem = Path(path).stem
        record = upref('wpt-html-tree-construction', rel)
        for index, t in enumerate(dat_tests(upbytes('wpt-html-tree-construction', rel).decode('utf-8'))):
            total += 1
            data = t['data'].encode('utf-8')
            if t['fragment'] or t['script'] == 'on' or not utf8(data) or '\x00' in t['data'] or '\r' in t['data']:
                continue
            cid = f'wpt-{stem}-{index:03}'
            doc, deriv = extracted('wpt-html-tree-construction', cid, data, 'html',
                                   f'#data of test {index} in {Path(path).name}, extracted byte-exact (final newline '
                                   'removed, as the html5lib .dat format specifies)')
            if not t['errors'] and not t['new_errors']:
                eligible += 1
                out.append(parser_case(case_id=cid, suite='wpt-html-tree-construction', language='html',
                                       dialect=DIALECT['html'], filename='fixture.html', doc=doc, derivation=deriv,
                                       record=record, accepts=True,
                                       plan={'oracle': 'html5lib_tree', 'file': rel, 'index': index},
                                       upstream_case=f'{Path(path).name}#{index}',
                                       label='test with an empty #errors section (a conforming document)',
                                       fields=['#data', '#errors', '#document']))
            seeds.append((cid, f'{Path(path).name}#{index}', record, doc, data))
    disc.append(discovery('wpt-html-tree-construction', 'html', 'parser_fidelity', total, eligible,
                          'document (not fragment, not scripting-on) tests with empty #errors/#new-errors: HTML has no '
                          'rejection, so only conforming documents carry a must-accept claim. Enumeration INCOMPLETE: '
                          'probed recalled .dat names'))
    d = Deriver('wpt-html-tree-construction', 'html', DIALECT['html'], 'fixture.html', 'html5lib_equal')
    A = 'HTML tree construction (scripting off); equivalence is an identical html5lib tree in the test format ' \
        '(attributes are sorted by the format, so source attribute order is outside the compared domain).'
    tag2 = re.compile(rb'<([a-z][a-z0-9]*)\s+(' + _ATTR + rb')\s+(' + _ATTR + rb')(?=[\s/>])')
    for seed in seeds:
        def swap(b):
            m = tag2.search(b)
            if not m or m.group(2).split(b'=')[0] == m.group(3).split(b'=')[0]:
                return None
            return b[:m.start(2)] + m.group(3) + b[m.end(2):m.start(3)] + m.group(2) + b[m.end(3):]
        d.add('attribute_order', seed, swap, 'preserved', 'first two attributes of the first start tag with two '
              'quoted attributes swapped', A, preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'],
                                                       'the element has the same attribute set'))
        d.add('attribute_quote', seed, sub_first(rb'(<[a-z][a-z0-9]*\s+(?:' + _ATTR + rb'\s+)*[a-zA-Z][a-zA-Z0-9-]*=)"([^"\'<>]*)"',
                                                 rb"\1'\2'"), 'preserved',
              'first double-quoted attribute value respelled with single quotes', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'the attribute value is identical'))
        d.add('attribute_value', seed, sub_first(rb'(<[a-z][a-z0-9]*\s+(?:' + _ATTR + rb'\s+)*[a-zA-Z][a-zA-Z0-9-]*=")([^"<>]*)"',
                                                 rb'\1\2x"'), 'changed', 'first double-quoted attribute value gains one character', A,
              changed(EDIT_TYPES, 'the DOM attribute value changes'))
        d.add('text_edit', seed, sub_first(rb'>([A-Za-z]{2,})(?=[<\s])', rb'>\1x'), 'changed',
              'first text run directly after a tag gains one letter', A, changed(EDIT_TYPES, 'the DOM text changes'))
        d.add('tag_case', seed, sub_first(rb'<(p|div|span|b|i|a|table|td|tr|li|ul|body|head|title)(?=[\s>])',
                                          lambda m: b'<' + m.group(1).upper()), 'preserved',
              'first start tag name of a common element upper-cased', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'tag names are ASCII case-insensitive'))
        d.add('tag_whitespace', seed, sub_first(rb'(<[a-z][a-z0-9]*)( ' + _ATTR + rb')(?=>)', rb'\1\2 '), 'preserved',
              'one space inserted before `>` of the first start tag ending with a quoted attribute', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'whitespace inside a tag is not content'))
    out += d.out
    disc.append(discovery('wpt-html-tree-construction', 'html', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits (own rows); seeds whose old-side '
                          'html5lib tree does not reproduce the upstream #document are not admitted'))
    return out, disc


# CSS (css-parsing-tests) ---------------------------------------------------------------------

def _has_error(value):
    if isinstance(value, list):
        return (len(value) == 2 and value[0] == 'error') or any(_has_error(v) for v in value)
    return False


def css_cases():
    out = []; disc = []
    seeds = []; total = 0; eligible = 0
    for name, mode in (('stylesheet.json', 'stylesheet'), ('rule_list.json', 'rule_list')):
        record = upref('css-parsing-tests', name)
        items = json.loads(upbytes('css-parsing-tests', name))
        for index in range(0, len(items), 2):
            total += 1
            source, expected = items[index], items[index + 1]
            try:
                data = source.encode('utf-8')
            except UnicodeEncodeError:
                continue
            if '\x00' in source or '\r' in source or '\f' in source:
                continue
            cid = f'csspt-{mode.replace("_", "")}-{index // 2:03}'
            doc, deriv = extracted('css-parsing-tests', cid, data, 'css',
                                   f'input string of test {index // 2} in {name} (JSON-decoded, UTF-8)')
            if not _has_error(expected):
                eligible += 1
                out.append(parser_case(case_id=cid, suite='css-parsing-tests', language='css', dialect=DIALECT['css'],
                                       filename='fixture.css', doc=doc, derivation=deriv, record=record, accepts=True,
                                       plan={'oracle': 'tinycss2_json', 'file': name, 'index': index // 2, 'mode': mode},
                                       upstream_case=f'{name}#{index // 2}',
                                       label='expected AST without an error node', fields=['input', 'expected']))
                if '{' in source and ':' in source:
                    seeds.append((cid, f'{name}#{index // 2}', record, doc, data))
    disc.append(discovery('css-parsing-tests', 'css', 'parser_fidelity', total, eligible,
                          'stylesheet.json and rule_list.json inputs whose expected AST has no error node (CSS recovers '
                          'from errors, so only error-free inputs carry a must-accept claim); inputs with NUL/CR/FF or '
                          'lone surrogates are excluded'))
    d = Deriver('css-parsing-tests', 'css', DIALECT['css'], 'fixture.css', 'tinycss2_equal')
    A = 'CSS Syntax Level 3; equivalence is an identical tinycss2 rule/declaration tree with comments skipped and ' \
        'whitespace runs collapsed (declaration values trimmed).'
    for seed in seeds:
        d.add('whitespace', seed, lambda b: re.sub(rb'([{;])(?=[^\s}])', rb'\1\n  ', b, count=3)
              if not re.search(rb'["\'\\]|url\(', b) else None, 'preserved',
              'newline + indentation inserted after the first three `{`/`;` (outside strings and urls)', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'formatting whitespace is not content'))
        d.add('comment', seed, sub_first(rb'\{', rb'{/* derived */') if True else None, 'preserved',
              'a comment inserted after the first `{`', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'comments are not part of the stylesheet'))
        d.add('value_edit', seed, sub_first(rb'(:\s*)([a-z]+|[0-9]+)(?=\s*[;}!])', rb'\1\2x'), 'changed',
              'first declaration value token gains one character', A, changed(EDIT_TYPES, 'the declaration value changes'))
    out += d.out
    disc.append(discovery('css-parsing-tests', 'css', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits of rule-bearing inputs (own rows)'))
    return out, disc


# SCSS (sass-spec) ----------------------------------------------------------------------------

def scss_cases():
    out = []; disc = []
    acq = acquisition('sass-spec')
    dirs = sorted({p.rsplit('/', 1)[0] for p in acq['files'] if p.endswith('/input.scss')})
    seeds = []; accept = 0; reject = 0
    for d in dirs:
        files = acq['files']
        data = upbytes('sass-spec', f'{d}/input.scss')
        if not utf8(data) or b'\r' in data:
            continue
        ref = upref('sass-spec', f'{d}/input.scss')
        cid = 'sass-' + d.removeprefix('package/spec/').replace('/', '-')
        if f'{d}/expected_output.css' in files and f'{d}/error' not in files:
            accept += 1
            out.append(parser_case(case_id=cid, suite='sass-spec', language='scss', dialect=DIALECT['scss'],
                                   filename='fixture.scss', doc=ref, derivation=S.VERBATIM, record=ref, accepts=True,
                                   plan={'oracle': 'sass_compile', 'dir': d}, upstream_case=d.removeprefix('package/'),
                                   label='expected_output.css (no error file)', fields=['input.scss', 'expected_output.css']))
            if not d.startswith('package/spec/errors'):
                seeds.append((cid, d.removeprefix('package/'), ref, ref, data))
        elif f'{d}/error' in files:
            err = upbytes('sass-spec', f'{d}/error').decode('utf-8', 'replace')
            if re.match(r'(Error: )?(expected|Invalid CSS after)', err.strip().split('\n')[0]):
                reject += 1
                out.append(parser_case(case_id=cid, suite='sass-spec', language='scss', dialect=DIALECT['scss'],
                                       filename='fixture.scss', doc=ref, derivation=S.VERBATIM, record=ref, accepts=False,
                                       plan={'oracle': 'sass_compile', 'dir': d}, upstream_case=d.removeprefix('package/'),
                                       label='error file with a syntax error message', fields=['input.scss', 'error']))
    disc.append(discovery('sass-spec', 'scss', 'parser_fidelity', acq.get('tarball_input_scss_total', len(dirs)),
                          accept + reject, 'retained spec/basic, spec/scss, spec/css and spec/errors directories: '
                          'expected_output.css = must accept; an `error` whose first line is a syntax error '
                          '("expected ..." / "Invalid CSS after") = must reject; other errors are semantic and excluded'))
    d = Deriver('sass-spec', 'scss', DIALECT['scss'], 'fixture.scss', 'sass_equal')
    A = 'Dart Sass expanded output; equivalence is identical compiled CSS.'
    for seed in seeds:
        d.add('silent_comment', seed, lambda b: b'// derived note\n' + b, 'preserved',
              'a silent `//` comment line prepended', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'silent comments are not emitted'))
        d.add('indentation', seed, lambda b: re.sub(rb'(?m)^(  +)(?=[a-z$@&.#-])', lambda m: m.group(1) * 2, b)
              if re.search(rb'(?m)^  +[a-z$@&.#-]', b) and not re.search(rb'["\'\\]|/\*', b) else None, 'preserved',
              'leading indentation doubled on every indented line (no strings or block comments in the seed)', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'indentation is not significant in SCSS'))
        d.add('number_edit', seed, sub_first(rb'(:\s*)([1-9][0-9]*)(px|em|%)', lambda m: m.group(1) + str(int(m.group(2)) + 1)
                                             .encode() + m.group(3)), 'changed',
              'first `: <n>px|em|%` value incremented by one', A, changed(EDIT_TYPES, 'the emitted value changes'))
        d.add('variable_value', seed, sub_first(rb'(?m)^(\$[a-z][a-z0-9_-]*:\s*)([a-z]+|[0-9]+)(?=\s*;)', rb'\1\2x'),
              'changed', 'first top-level variable assignment value gains one character', A,
              changed(EDIT_TYPES, 'the variable value changes'))
    out += d.out
    disc.append(discovery('sass-spec', 'scss', 'change_understanding', 0, 0,
                          'compilation pairs are not edits; only reviewed_external_derived edits (own rows)'))
    return out, disc


# Svelte ----------------------------------------------------------------------------------------

def svelte_cases():
    out = []; disc = []
    acq = acquisition('svelte-tests')
    base = 'packages/svelte/tests'
    seeds = []
    for kind in ('parser-modern', 'parser-legacy'):
        names = acq['enumeration'][f"{kind.replace('-', '_')}_found"]
        for n in names:
            path = f'{base}/{kind}/samples/{n}/input.svelte'
            data = upbytes('svelte-tests', path)
            if not utf8(data) or b'\r' in data:
                continue
            ref = upref('svelte-tests', path)
            has_output = f'{base}/{kind}/samples/{n}/output.json' in acq['files']
            loose = n.startswith('loose-')
            cid = f"svelte-{kind.split('-')[1]}-{n}"
            if has_output:
                out.append(parser_case(case_id=cid, suite='svelte-tests', language='svelte', dialect=DIALECT['svelte'],
                                       filename='fixture.svelte', doc=ref, derivation=S.VERBATIM, record=ref,
                                       accepts=not loose, plan={'oracle': 'svelte_parse', 'modern': kind == 'parser-modern'},
                                       upstream_case=f'{kind}/{n}',
                                       label='loose-mode (error-recovery) sample' if loose else 'sample with output.json AST',
                                       fields=['input.svelte', 'output.json']))
                if not loose:
                    seeds.append((cid, f'{kind}/{n}', ref, ref, data))
    disc.append(discovery('svelte-tests', 'svelte', 'parser_fidelity',
                          len(acq['enumeration']['parser_modern_found']) + len(acq['enumeration']['parser_legacy_found']),
                          sum(1 for c in out if c['lane'] == 'parser_fidelity'),
                          'parser-modern and parser-legacy samples with output.json; `loose-*` samples (parsed only in '
                          'loose mode) = must reject. Enumeration INCOMPLETE: probed recalled sample names'))
    pairs = 0
    for n in acq['enumeration']['migrate_found']:
        ip, op = f'{base}/migrate/samples/{n}/input.svelte', f'{base}/migrate/samples/{n}/output.svelte'
        if op not in acq['files']:
            continue
        old_b, new_b = upbytes('svelte-tests', ip), upbytes('svelte-tests', op)
        if old_b == new_b or not (utf8(old_b) and utf8(new_b)) or f'{base}/migrate/samples/{n}/_config.js' in acq['files'] \
                or new_b.startswith(b'<!-- @migration-task Error'):
            continue  # options/expected-error samples, and outputs the migrator itself labels as failed
        pairs += 1
        old, new = upref('svelte-tests', ip), upref('svelte-tests', op)
        out.append(case(case_id=f'svelte-migrate-{n}', suite='svelte-tests', language='svelte', dialect=DIALECT['svelte'],
                        filename='fixture.svelte', lane='change_understanding', family='migration_rewrite',
                        upstream_case=f'migrate/{n}', record=old, old=old, new=new,
                        derivation=[{'step': 'verbatim_upstream', 'description': 'upstream migrate input.svelte (old) and '
                                     'its expected output.svelte (new), unmodified'}],
                        claim='preserved', assumptions='Svelte 4 -> 5 migration; preserved = identical server-rendered '
                        'HTML with no props (svelte/server render), one bounded observation, not program equivalence.',
                        expected=(expect_preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'],
                                                   'the migration only removes whitespace inside tags')
                                  if re.sub(rb'\s', b'', old_b) == re.sub(rb'\s', b'', new_b) else
                                  expect_rewrite(old_b, new_b, 'the migration rewrites the component source')),
                        plan={'oracle': 'svelte_ssr_equal'}, review='Upstream migrate pair (task-defined migration).',
                        fields=['input.svelte', 'output.svelte']))
    disc.append(discovery('svelte-tests', 'svelte', 'change_understanding', len(acq['enumeration']['migrate_found']), pairs,
                          'migrate samples with output.svelte that differs from input.svelte, no _config.js (options '
                          'or expected-error samples excluded) and no `@migration-task Error` marker in the output; a '
                          'whitespace-only migration is a preservation control, any other is an equivalent rewrite. Enumeration INCOMPLETE: probed recalled names'))
    d = Deriver('svelte-tests', 'svelte', DIALECT['svelte'], 'fixture.svelte', 'svelte_ssr_equal')
    A = 'Svelte 5.57.3 server render with no props; equivalence is identical rendered body and head HTML.'

    def markup_only(fn):
        def apply(b):
            # never touch <script>/<style> contents
            cut = re.search(rb'<(script|style)\b', b)
            head, tail = (b, b'') if not cut else (b[:cut.start()], b[cut.start():])
            r = fn(head)
            return None if r is None else r + tail
        return apply
    for seed in seeds:
        d.add('text_edit', seed, markup_only(sub_first(rb'>([A-Za-z]{2,})(?=[<\s])', rb'>\1x')), 'changed',
              'first markup text run directly after a tag gains one letter (before any script/style)', A,
              changed(EDIT_TYPES, 'the rendered text changes'))
        d.add('script_quote', seed, sub_first(rb"(<script[^>]*>[^<]*?)'([A-Za-z0-9 _.-]*)'", rb'\1"\2"'), 'preserved',
              'first single-quoted script string literal (plain characters) respelled with double quotes', A,
              preserved(['ADDITION', 'DELETION', 'MOVE'], 'the string value is identical'))
        d.add('tag_whitespace', seed, markup_only(sub_first(rb'(<[a-z][a-z0-9]*)( [a-z][a-z0-9:-]*=)', rb'\1 \2')),
              'preserved', 'one extra space before the first attribute of the first element with an attribute', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'whitespace between attributes is not content'))
    out += d.out
    return out, disc


# Vue -------------------------------------------------------------------------------------------

_TPL = re.compile(r'(?:parse|compile|compileSFCScript)\(\s*`((?:[^`\\$]|\$(?!\{))*)`', re.S)


def vue_sources():
    out = []
    for path in ('packages/compiler-sfc/__tests__/parse.spec.ts', 'packages/compiler-sfc/__tests__/compileScript.spec.ts'):
        text = upbytes('vue-compiler-sfc-tests', path).decode('utf-8')
        for index, m in enumerate(_TPL.finditer(text)):
            src = m.group(1)
            if '<' not in src:
                continue
            out.append((path, index, src))
    return out


def vue_cases():
    out = []; disc = []
    seeds = []; sources = vue_sources()
    seen = set()
    for path, index, src in sources:
        data = src.encode('utf-8')
        if data in seen:
            continue
        seen.add(data)
        stem = 'parse' if 'parse.spec' in path else 'script'
        cid = f'vue-{stem}-{index:03}'
        doc, deriv = extracted('vue-compiler-sfc-tests', cid, data, 'vue',
                               f'template literal #{index} passed to parse/compile in {Path(path).name}, extracted '
                               'byte-exact (literals with `${` or backslash escapes are skipped)')
        record = upref('vue-compiler-sfc-tests', path)
        out.append(parser_case(case_id=cid, suite='vue-compiler-sfc-tests', language='vue', dialect=DIALECT['vue'],
                               filename='fixture.vue', doc=doc, derivation=deriv, record=record, accepts=True,
                               plan={'oracle': 'vue_compile'}, upstream_case=f'{Path(path).name}#{index}',
                               label='SFC source used by a compiler-sfc test', fields=[path]))
        if '<template' in src and 'lang="ts"' not in src and "lang='ts'" not in src:
            seeds.append((cid, f'{Path(path).name}#{index}', record, doc, data))
    disc.append(discovery('vue-compiler-sfc-tests', 'vue', 'parser_fidelity', len(sources), len(seen),
                          'distinct escape-free template literals passed to parse()/compile() in parse.spec.ts and '
                          'compileScript.spec.ts; must accept (some upstream tests assert errors: the reference '
                          'reporting errors leaves those cases candidates)'))
    d = Deriver('vue-compiler-sfc-tests', 'vue', DIALECT['vue'], 'fixture.vue', 'vue_ssr_equal')
    A = 'Vue 3.5.45 SFC compiled for SSR and rendered with vue/server-renderer (no props); equivalence is identical HTML.'

    def template_only(fn):
        def apply(b):
            m = re.search(rb'<template>(.*?)</template>', b, re.S)
            if not m:
                return None
            r = fn(m.group(1))
            return None if r is None else b[:m.start(1)] + r + b[m.end(1):]
        return apply
    for seed in seeds:
        d.add('text_edit', seed, template_only(sub_first(rb'>([A-Za-z]{2,})(?=[<\s])', rb'>\1x')), 'changed',
              'first template text run directly after a tag gains one letter', A,
              changed(EDIT_TYPES, 'the rendered text changes'))
        d.add('script_quote', seed, sub_first(rb"(<script[^>]*>[^<]*?)'([A-Za-z0-9 _.-]*)'", rb'\1"\2"'), 'preserved',
              'first single-quoted script string literal (plain characters) respelled with double quotes', A,
              preserved(['ADDITION', 'DELETION', 'MOVE'], 'the string value is identical'))
        d.add('tag_whitespace', seed, template_only(sub_first(rb'(<[a-z][a-z0-9]*)( [a-z][a-z0-9:@.-]*=)', rb'\1 \2')),
              'preserved', 'one extra space before the first attribute of the first template element with one', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'whitespace between attributes is not content'))
    out += d.out
    disc.append(discovery('vue-compiler-sfc-tests', 'vue', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits (own rows)'))
    return out, disc


# Astro -----------------------------------------------------------------------------------------

_GO_SOURCE = re.compile(r'\n\t+name: +"((?:[^"\\]|\\.)*)",\n\t+source: +`([^`]*)`,\n')


def astro_cases():
    out = []; disc = []
    path = 'internal/printer/printer_test.go'
    text = upbytes('astro-compiler-tests', path).decode('utf-8')
    record = upref('astro-compiler-tests', path)
    found = _GO_SOURCE.findall(text)
    seeds = []; seen = set()
    for index, (name, src) in enumerate(found):
        data = src.encode('utf-8')
        if data in seen or not data.strip() or '\r' in src:
            continue
        seen.add(data)
        cid = f'astro-printer-{index:03}'
        doc, deriv = extracted('astro-compiler-tests', cid, data, 'astro',
                               f'Go raw-string `source` of printer test #{index} ("{name}"), byte-exact (raw strings have '
                               'no escapes; tests whose source is concatenated are skipped)')
        out.append(parser_case(case_id=cid, suite='astro-compiler-tests', language='astro', dialect=DIALECT['astro'],
                               filename='fixture.astro', doc=doc, derivation=deriv, record=record, accepts=True,
                               plan={'oracle': 'astro_transform'}, upstream_case=f'printer_test.go: {name}',
                               label='printer test source with an expected compiled output', fields=['source', 'want']))
        seeds.append((cid, name, record, doc, data))
    disc.append(discovery('astro-compiler-tests', 'astro', 'parser_fidelity', text.count('source:'), len(seen),
                          'distinct, non-empty single raw-string `source` fields of the printer tests; must accept'))
    d = Deriver('astro-compiler-tests', 'astro', DIALECT['astro'], 'fixture.astro', 'astro_transform_equal')
    A = '@astrojs/compiler 4.0.0 transform (default options, fixed filename); equivalence is identical compiled module.'
    for seed in seeds:
        d.add('text_edit', seed, sub_first(rb'>([A-Za-z]{2,})(?=[<\s])', rb'>\1x'), 'changed',
              'first template text run directly after a tag gains one letter', A,
              changed(EDIT_TYPES, 'the rendered text changes'))
        d.add('attribute_value', seed, sub_first(rb'(<[a-z][a-z0-9]*\s+[a-z][a-z0-9-]*=")([A-Za-z0-9 _-]+)"', rb'\1\2x"'),
              'changed', 'first static double-quoted attribute value gains one character', A,
              changed(EDIT_TYPES, 'the rendered attribute changes'))
        d.add('frontmatter_blank_line', seed, sub_first(rb'\A---\n', b'---\n\n'), 'preserved',
              'one blank line inserted at the start of the frontmatter', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'a blank line in frontmatter is not code'))
    out += d.out
    disc.append(discovery('astro-compiler-tests', 'astro', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits (own rows)'))
    return out, disc


# AsciiDoc ----------------------------------------------------------------------------------------

def asciidoc_examples(text):
    """DocTest example files: each example starts with a `// .name` line and runs to the next one."""
    out = []; name = None; buf = []
    for line in text.splitlines(keepends=True):
        m = re.match(r'// \.([A-Za-z0-9_-]+)\s*$', line)
        if m:
            if name is not None:
                out.append((name, ''.join(buf)))
            name, buf = m.group(1), []
        elif name is not None:
            buf.append(line)
    if name is not None:
        out.append((name, ''.join(buf)))
    return [(n, b.rstrip('\n') + '\n') for n, b in out if b.strip()]


def asciidoc_cases():
    out = []; disc = []
    acq = acquisition('asciidoctor-doctest')
    seeds = []; total = 0
    for path in acq['enumeration']['found']:
        record = upref('asciidoctor-doctest', path)
        group = Path(path).stem
        for name, body in asciidoc_examples(upbytes('asciidoctor-doctest', path).decode('utf-8')):
            total += 1
            cid = f'adoc-{group}-{name}'.replace('_', '-')
            doc, deriv = extracted('asciidoctor-doctest', cid, body.encode('utf-8'), 'adoc',
                                   f'example `.{name}` of {Path(path).name}: the lines after its `// .{name}` marker up to '
                                   'the next marker, trailing blank lines normalised to one newline')
            out.append(parser_case(case_id=cid, suite='asciidoctor-doctest', language='asciidoc', dialect=DIALECT['asciidoc'],
                                   filename='fixture.adoc', doc=doc, derivation=deriv, record=record, accepts=True,
                                   plan={'oracle': 'asciidoctor_convert'}, upstream_case=f'{group}:{name}',
                                   label='DocTest reference example', fields=[path]))
            seeds.append((cid, f'{group}:{name}', record, doc, body.encode('utf-8')))
    disc.append(discovery('asciidoctor-doctest', 'asciidoc', 'parser_fidelity', total, total,
                          'every example of the retained example files; must accept (converted without warnings). '
                          'Enumeration INCOMPLETE: probed recalled file names (block_* files were not found)'))
    d = Deriver('asciidoctor-doctest', 'asciidoc', DIALECT['asciidoc'], 'fixture.adoc', 'asciidoctor_equal')
    A = 'Asciidoctor 2.0.26 html5 embedded output, no attributes set; equivalence is identical HTML.'
    for seed in seeds:
        d.add('link_target', seed, sub_first(rb'(https?://[A-Za-z0-9./_-]*[A-Za-z0-9])', rb'\1x'), 'changed',
              'first URL gains one character', A, changed(EDIT_TYPES, 'the link target changes'))
        d.add('word_edit', seed, sub_first(rb'\b(chunky|bacon|text|word|Hello|hello|world)\b', rb'\1x'), 'changed',
              'first common example word gains one letter', A, changed(EDIT_TYPES, 'rendered text changes'))
        d.add('comment_line', seed, lambda b: b + b'\n// derived comment\n', 'preserved',
              'a line comment appended after a blank line', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'line comments are not rendered'))
    out += d.out
    disc.append(discovery('asciidoctor-doctest', 'asciidoc', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits (own rows)'))
    return out, disc


# PO ------------------------------------------------------------------------------------------

_MSGSTR = re.compile(rb'(?m)^msgstr "([^"\\\n]{4,})"\n')


def po_cases():
    out = []; disc = []
    acq = acquisition('polib-tests')
    paths = [p for p in acq['enumeration']['found'] if p.endswith(('.po', '.pot'))]
    seeds = []; eligible = 0
    for p in paths:
        data = upbytes('polib-tests', p)
        if not utf8(data) or b'\r' in data:
            continue
        eligible += 1
        ref = upref('polib-tests', p)
        name = Path(p).stem
        bad = 'syntax_error' in name or 'garbage' in name
        cid = 'po-' + name.removeprefix('test_').replace('_', '-')
        out.append(parser_case(case_id=cid, suite='polib-tests', language='po', dialect=DIALECT['po'],
                               filename='fixture.pot' if p.endswith('.pot') else 'fixture.po', doc=ref,
                               derivation=S.VERBATIM, record=ref, accepts=not bad, plan={'oracle': 'msgfmt_check'},
                               upstream_case=p, label=f'file name `{Path(p).name}` ({"syntax-error fixture" if bad else "valid fixture"})',
                               fields=[p]))
        if not bad and p.endswith('.po'):
            seeds.append((cid, p, ref, ref, data))
    disc.append(discovery('polib-tests', 'po', 'parser_fidelity', len(paths), eligible,
                          'UTF-8 .po/.pot fixtures; names containing syntax_error/garbage = must reject. Enumeration '
                          'INCOMPLETE: files named in tests/tests.py plus probed names'))
    d = Deriver('polib-tests', 'po', DIALECT['po'], 'fixture.po', 'msgfmt_equal')
    A = 'GNU msgfmt 0.21 compiled catalogue (fuzzy entries excluded, the default); equivalence is identical .mo bytes.'

    def split_msgstr(b):
        m = _MSGSTR.search(b)
        if not m:
            return None
        v = m.group(1); half = len(v.decode('utf-8')[:len(v.decode('utf-8')) // 2].encode('utf-8'))
        return b[:m.start()] + b'msgstr ""\n"' + v[:half] + b'"\n"' + v[half:] + b'"\n' + b[m.end():]
    for seed in seeds:
        d.add('msgstr_edit', seed, sub_first(rb'(?m)^(msgstr "[^"\\\n]{4,})"\n', rb'\1x"\n'), 'changed',
              'first non-empty single-line msgstr gains one character', A, changed(EDIT_TYPES, 'the translation changes'))
        d.add('msgstr_split', seed, split_msgstr, 'preserved',
              'first single-line msgstr split into two adjacent quoted pieces', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'adjacent pieces concatenate to the same string'))
        d.add('translator_comment', seed, sub_first(rb'(?m)^msgid "[^"\n]', lambda m: b'# derived translator note\n' + m.group(0)),
              'preserved', 'a translator comment line inserted before the first msgid', A,
              preserved(['DELETION', 'MODIFICATION', 'MOVE'], 'translator comments are not compiled'))
    out += d.out
    disc.append(discovery('polib-tests', 'po', 'change_understanding', 0, 0,
                          'merge fixtures are not reviewed edit pairs; only reviewed_external_derived edits (own rows)'))
    return out, disc


# LaTeX ---------------------------------------------------------------------------------------

def latex_cases():
    out = []; disc = []
    acq = acquisition('latex2e-base')
    paths = sorted(p for p in acq['files'] if p.endswith('.tex'))
    seeds = []
    for p in paths:
        data = upbytes('latex2e-base', p)
        if not utf8(data) or b'\r' in data or b'\\documentclass' not in data:
            continue
        ref = upref('latex2e-base', p)
        cid = 'latex-' + Path(p).stem
        out.append(parser_case(case_id=cid, suite='latex2e-base', language='latex', dialect=DIALECT['latex'],
                               filename='fixture.tex', doc=ref, derivation=S.VERBATIM, record=ref, accepts=True,
                               plan={'oracle': 'latex_compile'}, upstream_case=p,
                               label='distribution document listed in base/manifest.txt', fields=[p]))
        seeds.append((cid, p, ref, ref, data))
    disc.append(discovery('latex2e-base', 'latex', 'parser_fidelity', len(acq['enumeration']['listed']), len(out),
                          'UTF-8 documents with \\documentclass among the base/manifest.txt .tex entries; must accept '
                          '(compiled without error; documents needing packages absent from texlive-latex-base are '
                          'inconclusive and stay candidates). The l3build testfiles are not imported'))
    d = Deriver('latex2e-base', 'latex', DIALECT['latex'], 'fixture.tex', 'latex_dvi_equal')
    A = 'pdfTeX in DVI mode, two runs, SOURCE_DATE_EPOCH=0 with FORCE_SOURCE_DATE=1; equivalence is identical DVI bytes.'
    for seed in seeds:
        d.add('comment_line', seed, sub_first(rb'(?m)^(\\documentclass[^\n]*\n)', rb'\1% derived comment\n'), 'preserved',
              'a full-line comment inserted after \\documentclass', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'a full-line comment is not typeset'))
        d.add('section_level', seed, sub_first(rb'\\section\{', rb'\\subsection{'), 'changed',
              'first \\section becomes \\subsection', A, changed(EDIT_TYPES, 'the sectioning level changes'))
        d.add('interword_space', seed, sub_first(rb'(?m)^([A-Z][a-z]+) ([a-z]{2,} )', rb'\1  \2'), 'preserved',
              'one extra space between the first two words of a prose line', A,
              preserved(['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE'], 'TeX collapses spaces'))
        d.add('word_edit', seed, sub_first(rb'(?m)^([A-Z][a-z]{3,})(?= [a-z])', rb'\1x'), 'changed',
              'first capitalised word that starts a prose line gains one letter', A,
              changed(EDIT_TYPES, 'typeset text changes'))
    out += d.out
    disc.append(discovery('latex2e-base', 'latex', 'change_understanding', 0, 0,
                          'no before/after pairs; only reviewed_external_derived edits (own rows)'))
    return out, disc


BUILDERS = {'markdown': markdown_cases, 'mdx': mdx_cases, 'html': html_cases, 'css': css_cases, 'scss': scss_cases,
            'svelte': svelte_cases, 'vue': vue_cases, 'astro': astro_cases, 'asciidoc': asciidoc_cases, 'po': po_cases,
            'latex': latex_cases}


def suite_records():
    out = []
    for suite, (name, version, url, evidence, _) in SUITE_INFO.items():
        acq = acquisition(suite)
        licence, licence_evidence, attribution = LIC[suite]
        verification = [f"revision: {acq['revision_resolution']}",
                        f"{len(acq['files'])} files retained with SHA-256 in ACQUISITION.json",
                        f"enumeration: {acq['enumeration']['method']} (complete: {acq['enumeration'].get('complete')})",
                        'BLOCKED: github.com tree listings, the GitHub REST API and git clones are not reachable for this '
                        'repository from the sandbox; no tree-level cross-check was possible'] + acq.get('notes', [])
        if acq.get('npm'):
            n = acq['npm']
            verification.append(f"npm {n['package']}@{n['version']}: tarball SHA-256 {n['tarball_sha256']}, integrity "
                                f"{n['integrity']}, gitHead {n['gitHead']}")
        out.append({'suite_id': suite, 'name': name, 'version': version, 'url': url, 'revision': acq['revision'],
                    'licence': licence, 'licence_evidence': licence_evidence, 'attribution': attribution,
                    'evidence_class': evidence,
                    'acquisition': {'method': 'scripts/acquire_markup_benchmarks.py (raw.githubusercontent.com at the '
                                              'pinned commit, or the npm tarball whose gitHead is that commit)',
                                    'files': [upref(suite, 'ACQUISITION.json')], 'verification': verification}})
    return out


def define(languages=None):
    cases = []; disc = []
    for lang, fn in BUILDERS.items():
        if languages and lang not in languages:
            continue
        c, d = fn(); cases += c; disc += d
    return cases, disc


def build():
    cases, disc = define()
    ids = Counter(c['id'] for c in cases)
    dup = [k for k, n in ids.items() if n > 1]
    if dup:
        raise ValueError(f'duplicate case ids: {dup[:5]}')
    return {'schema_version': 1, 'family': 'core#263 web, markup and embedded languages', 'suites': suite_records(),
            'discovery': disc, 'cases': cases, 'blocked_acquisitions': BLOCKED}


# --- witness execution -----------------------------------------------------------------------

NODE_REF = r'''
import { readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { pathToFileURL } from 'node:url';
const jobs = JSON.parse(readFileSync(0, 'utf8'));
const h = s => createHash('sha256').update(s).digest('hex');
const tmp = process.env.MARKUP_TMP; mkdirSync(tmp, { recursive: true });
let n = 0;
const out = [];
const mod = {};
async function load(name) { return mod[name] ??= await import(name); }
for (const job of jobs) {
  const r = { key: job.key };
  try {
    if (job.mode === 'commonmark') {
      const cm = (await load('commonmark')).default ?? await load('commonmark');
      const doc = new cm.Parser().parse(job.src);
      r.html = new cm.HtmlRenderer().render(doc);
      r.blocks = []; let c = doc.firstChild;
      while (c) { r.blocks.push({ type: c.type, start_line: c.sourcepos[0][0], end_line: c.sourcepos[1][0] }); c = c.next; }
      r.version = JSON.parse(readFileSync(process.env.MARKUP_NM + '/commonmark/package.json')).version;
    } else if (job.mode === 'sass') {
      const sass = await load('sass');
      r.version = sass.info;
      try { r.css = sass.compileString(job.src, { style: 'expanded', syntax: 'scss', quietDeps: true, logger: sass.Logger.silent }).css; r.ok = true; }
      catch (e) { r.ok = false; r.error = String(e.sassMessage ?? e.message).slice(0, 300); }
    } else if (job.mode === 'svelte_parse') {
      const svelte = await load('svelte/compiler'); r.version = svelte.VERSION;
      try { svelte.parse(job.src, { modern: job.modern }); r.ok = true; } catch (e) { r.ok = false; r.error = String(e.message).slice(0, 300); }
    } else if (job.mode === 'svelte_ssr') {
      const svelte = await load('svelte/compiler'); r.version = svelte.VERSION;
      const js = svelte.compile(job.src, { generate: 'server', filename: 'Fixture.svelte', dev: false }).js.code;
      const file = `${tmp}/c${n++}.mjs`; writeFileSync(file, js);
      const { render } = await load('svelte/server');
      const Comp = (await import(pathToFileURL(file).href)).default;
      const res = render(Comp, { props: {} });
      r.html = res.body; r.head = res.head; r.ok = true;
    } else if (job.mode === 'vue_parse' || job.mode === 'vue_ssr') {
      const sfc = await load('@vue/compiler-sfc'); r.version = sfc.version;
      const { descriptor, errors } = sfc.parse(job.src, { filename: 'Fixture.vue' });
      r.errors = errors.map(e => String(e.message ?? e).slice(0, 200));
      if (job.mode === 'vue_parse') {
        if (!errors.length) {
          // compile the parsed blocks so a compiler error also counts against acceptance
          if (descriptor.script || descriptor.scriptSetup) sfc.compileScript(descriptor, { id: 'x' });
          if (descriptor.template && !descriptor.scriptSetup) {
            const t = sfc.compileTemplate({ source: descriptor.template.content, filename: 'Fixture.vue', id: 'x' });
            r.errors.push(...t.errors.map(e => String(e.message ?? e).slice(0, 200)));
          }
        }
        r.ok = r.errors.length === 0;
      } else {
        if (errors.length) throw new Error('parse errors: ' + r.errors.join('; '));
        let code;
        if (descriptor.script || descriptor.scriptSetup) {
          code = sfc.compileScript(descriptor, { id: 'x', inlineTemplate: true, templateOptions: { ssr: true } }).content;
          if (!descriptor.scriptSetup && descriptor.template) {
            const t = sfc.compileTemplate({ source: descriptor.template.content, filename: 'Fixture.vue', id: 'x', ssr: true });
            code = code.replace('export default', 'const __c =') + '\n' + t.code + '\n__c.ssrRender = ssrRender\nexport default __c\n';
          }
        } else {
          const t = sfc.compileTemplate({ source: descriptor.template.content, filename: 'Fixture.vue', id: 'x', ssr: true });
          code = t.code + '\nexport default { ssrRender }\n';
        }
        const file = `${tmp}/v${n++}.mjs`; writeFileSync(file, code);
        const vue = await load('vue'); const sr = await load('vue/server-renderer');
        const Comp = (await import(pathToFileURL(file).href)).default;
        const app = vue.createSSRApp(Comp); app.config.warnHandler = () => {};
        r.html = await sr.renderToString(app); r.ok = true;
      }
    } else if (job.mode === 'astro') {
      const astro = await load('@astrojs/compiler');
      const res = await astro.transform(job.src, { filename: '/fixture/Fixture.astro', sourcemap: false });
      r.diagnostics = res.diagnostics.map(d => ({ severity: d.severity, text: String(d.text).slice(0, 200) }));
      r.code_sha256 = h(res.code); r.ok = !res.diagnostics.some(d => d.severity === 1);
      r.version = JSON.parse(readFileSync(process.env.MARKUP_NM + '/@astrojs/compiler/package.json')).version;
    } else if (job.mode === 'mdx') {
      const mdx = await load('@mdx-js/mdx');
      try { const f = await mdx.compile(job.src); r.code_sha256 = h(String(f)); r.ok = true; }
      catch (e) { r.ok = false; r.error = String(e.message).slice(0, 300); }
    } else throw new Error('unknown mode ' + job.mode);
  } catch (e) { r.crash = String(e && e.stack || e).slice(0, 400); }
  out.push(r);
}
rmSync(tmp, { recursive: true, force: true });
writeFileSync(process.env.MARKUP_OUT, JSON.stringify(out));
'''

PY_REF = r'''
import json, sys
jobs = json.load(sys.stdin); out = []
for job in jobs:
    r = {'key': job['key']}
    try:
        if job['mode'] == 'html5lib':
            import html5lib
            p = html5lib.HTMLParser(tree=html5lib.getTreeBuilder('dom'), namespaceHTMLElements=False)
            doc = p.parse(job['src'])
            # html5lib's own test-harness conversion: drop the '#document' line, one space less after '|'
            lines = p.tree.testSerializer(doc).split('\n')
            r['tree'] = '\n'.join('|' + l[2:] if l.startswith('|') else l for l in lines if l != '#document'); r['errors'] = len(p.errors); r['version'] = html5lib.__version__
        elif job['mode'] == 'gfm':
            import cmarkgfm
            from cmarkgfm.cmark import Options
            r['html'] = cmarkgfm.github_flavored_markdown_to_html(job['src'], options=Options.CMARK_OPT_UNSAFE)
            import importlib.metadata as m; r['version'] = 'cmarkgfm ' + m.version('cmarkgfm')
        elif job['mode'] in ('tinycss2', 'tinycss2_rules'):
            import tinycss2
            import importlib.metadata as m; r['version'] = 'tinycss2 ' + m.version('tinycss2')
            def ser(nodes): return [node(n) for n in nodes]
            def num(n): return n.representation, n.value, 'integer' if n.is_integer else 'number'
            def node(n):
                t = n.type
                if t == 'qualified-rule': return ['qualified rule', ser(n.prelude), ser(n.content)]
                if t == 'at-rule': return ['at-rule', n.at_keyword, ser(n.prelude), None if n.content is None else ser(n.content)]
                if t == 'declaration': return ['declaration', n.name, ser(n.value), n.important]
                if t == 'whitespace': return ' '
                if t == 'literal': return n.value
                if t in ('ident', 'at-keyword', 'string', 'url'): return [t, n.value]
                if t == 'hash': return ['hash', n.value, 'id' if n.is_identifier else 'unrestricted']
                if t == 'number': return ['number', *num(n)]
                if t == 'percentage': return ['percentage', *num(n)]
                if t == 'dimension': return ['dimension', *num(n), n.unit]
                if t == 'unicode-range': return ['unicode-range', n.start, n.end]
                if t in ('() block', '[] block', '{} block'): return [t[:2], *ser(n.content)]
                if t == 'function': return ['function', n.name, *ser(n.arguments)]
                if t == 'comment': return '/* … */'
                if t == 'error': return ['error', n.kind]
                raise TypeError(t)
            parse = tinycss2.parse_stylesheet if job.get('rules') != 'rule_list' else tinycss2.parse_rule_list
            rules = parse(job['src'], skip_comments=True, skip_whitespace=True)
            r['json'] = ser(rules)
            def canon_tokens(tokens):
                out, ws = [], False
                for t in tokens:
                    if t.type == 'comment': continue
                    if t.type == 'whitespace': ws = True; continue
                    if ws and out: out.append(' ')
                    ws = False; out.append(node(t) if not hasattr(t, 'content') or t.type == 'function' else [t.type[:2], canon_tokens(t.content)])
                return out
            def canon(rule):
                if rule.type == 'error': return ['error', rule.kind]
                body = None
                if getattr(rule, 'content', None) is not None:
                    items = tinycss2.parse_blocks_contents(rule.content, skip_comments=True, skip_whitespace=True)
                    body = [['declaration', d.name, canon_tokens(d.value), d.important] if d.type == 'declaration'
                            else canon(d) if d.type in ('qualified-rule', 'at-rule') else ['error', getattr(d, 'kind', d.type)]
                            for d in items]
                return [rule.type, getattr(rule, 'at_keyword', None), canon_tokens(rule.prelude), body]
            r['canonical'] = [canon(x) for x in rules]
        else:
            raise ValueError(job['mode'])
    except Exception as e:
        r['crash'] = (type(e).__name__ + ': ' + str(e))[:300]
    out.append(r)
json.dump(out, sys.stdout)
'''


def _node(jobs):
    if not jobs:
        return {}
    nm = Path(TOOLS['node_modules'])
    script = nm.parent / '.markup-witness.mjs'
    script.write_text(NODE_REF)
    result = nm.parent / '.markup-witness-out.json'
    try:
        r = subprocess.run(['node', str(script)], input=json.dumps(jobs), capture_output=True, text=True, timeout=1800,
                           env={**os.environ, 'MARKUP_TMP': str(nm.parent / '.markup-witness-tmp'),
                                'MARKUP_OUT': str(result), 'MARKUP_NM': str(nm)})
        if r.returncode:
            raise RuntimeError('node reference failed: ' + r.stderr[-800:])
        return {x['key']: x for x in json.loads(result.read_text())}
    finally:
        script.unlink(missing_ok=True); result.unlink(missing_ok=True)


def _py(jobs):
    if not jobs:
        return {}
    r = subprocess.run([TOOLS['python'], '-I', '-c', PY_REF], input=json.dumps(jobs), capture_output=True, text=True,
                       timeout=1800)
    if r.returncode:
        raise RuntimeError('python reference failed: ' + r.stderr[-800:])
    return {x['key']: x for x in json.loads(r.stdout)}


def _asciidoctor(src):
    r = subprocess.run([TOOLS['asciidoctor'], '-s', '-a', 'nofooter', '-o', '-', '-'], input=src, capture_output=True,
                       timeout=60)
    return {'ok': r.returncode == 0 and not r.stderr.strip(), 'html_sha256': hashlib.sha256(r.stdout).hexdigest(),
            'stderr': r.stderr.decode(errors='replace')[-300:] or None}


def _msgfmt(data):
    with tempfile.TemporaryDirectory() as tmp:
        (Path(tmp) / 'f.po').write_bytes(data)
        r = subprocess.run([TOOLS['msgfmt'], '--check', '-o', 'f.mo', 'f.po'], cwd=tmp, capture_output=True, timeout=60)
        mo = (Path(tmp) / 'f.mo').read_bytes() if r.returncode == 0 else None
    return {'ok': r.returncode == 0, 'mo_sha256': hashlib.sha256(mo).hexdigest() if mo is not None else None,
            'stderr': r.stderr.decode(errors='replace')[-300:] or None}


def _latex(data):
    with tempfile.TemporaryDirectory() as tmp:
        (Path(tmp) / 'doc.tex').write_bytes(data)
        env = {**os.environ, 'SOURCE_DATE_EPOCH': '0', 'FORCE_SOURCE_DATE': '1'}
        log = ''
        for _ in range(2):
            r = subprocess.run([TOOLS['latex'], '-interaction=nonstopmode', '-halt-on-error', 'doc.tex'], cwd=tmp,
                               capture_output=True, timeout=180, env=env)
            log = (Path(tmp) / 'doc.log').read_text(errors='replace') if (Path(tmp) / 'doc.log').exists() else ''
            if r.returncode:
                break
        dvi = Path(tmp) / 'doc.dvi'
        missing = re.findall(r"! LaTeX Error: File `([^']+)' not found", log) + re.findall(r"! Font \S+=(\S+) not loadable", log)
        return {'ok': r.returncode == 0 and dvi.exists(), 'missing': missing or None,
                'dvi_sha256': hashlib.sha256(dvi.read_bytes()).hexdigest() if r.returncode == 0 and dvi.exists() else None,
                'error': (re.search(r'^! .*$', log, re.M) or [None])[0]}


def _record(c, verdict, tool, obs, summary):
    return {'case_id': c['id'], 'old_sha256': c['old']['sha256'], 'new_sha256': c['new']['sha256'],
            'verdict': verdict, 'oracle': c['witness_plan']['oracle'], 'reference': tool, 'observations': obs,
            'summary': summary, 'executed_utc': datetime.now(timezone.utc).isoformat(timespec='seconds')}


def _blocks_inventory(src: bytes, blocks):
    """commonmark.js top-level blocks -> start bytes of their first lines (if they cover every non-blank line)."""
    lines = src.split(b'\n')
    starts_of_line = [0]
    for line in lines:
        starts_of_line.append(starts_of_line[-1] + len(line) + 1)
    covered = set()
    for b in blocks:
        covered.update(range(b['start_line'], b['end_line'] + 1))
    for number, line in enumerate(lines, start=1):
        if line.strip(b' \t') and number not in covered:
            return None  # e.g. link reference definitions: consumed, no AST block
    starts = sorted({starts_of_line[b['start_line'] - 1] for b in blocks})
    if len(starts) != len(blocks) or any(s >= len(src) for s in starts):
        return None
    return {'count': len(starts), 'starts': starts,
            'evidence': 'commonmark.js 0.31.2 top-level block sourcepos (first line start byte); every non-blank line '
                        'belongs to a top-level block'}


ORACLE_MODE = {'commonmark_example': 'commonmark', 'commonmark_render': 'commonmark', 'sass_compile': 'sass',
               'sass_equal': 'sass', 'svelte_parse': 'svelte_parse', 'svelte_ssr_equal': 'svelte_ssr',
               'vue_compile': 'vue_parse', 'vue_ssr_equal': 'vue_ssr', 'astro_transform': 'astro',
               'astro_transform_equal': 'astro', 'mdx_compile': 'mdx', 'mdx_compile_equal': 'mdx',
               'html5lib_tree': 'html5lib', 'html5lib_equal': 'html5lib', 'gfm_example': 'gfm',
               'tinycss2_json': 'tinycss2', 'tinycss2_equal': 'tinycss2'}
NODE_MODES = {'commonmark', 'sass', 'svelte_parse', 'svelte_ssr', 'vue_parse', 'vue_ssr', 'astro', 'mdx'}


def observe(cases):
    """Run every reference once per (mode, document); returns {(mode, sha, extra): observation}."""
    node_jobs, py_jobs, local = {}, {}, {}
    for c in cases:
        oracle = c['witness_plan']['oracle']
        sides = [c['old']] if c['lane'] == 'parser_fidelity' else [c['old'], c['new']]
        for side in sides:
            data = (ROOT / side['path']).read_bytes()
            mode = ORACLE_MODE.get(oracle, oracle)
            extra = c['witness_plan'].get('modern') if mode == 'svelte_parse' else \
                c['witness_plan'].get('mode') if mode == 'tinycss2' else None
            key = f"{mode}|{side['sha256']}|{extra}"
            job = {'key': key, 'mode': mode, 'src': data.decode('utf-8')}
            if mode == 'svelte_parse':
                job['modern'] = bool(extra)
            if mode == 'tinycss2':
                job['rules'] = extra
            if mode in NODE_MODES:
                node_jobs[key] = job
            elif mode in ('html5lib', 'gfm', 'tinycss2'):
                py_jobs[key] = job
            else:
                local[key] = (mode, data)
    obs = {}
    # SSR jobs import generated modules: run them in their own process, in chunks.
    ssr = [j for j in node_jobs.values() if j['mode'] in ('svelte_ssr', 'vue_ssr')]
    rest = [j for j in node_jobs.values() if j['mode'] not in ('svelte_ssr', 'vue_ssr')]
    obs.update(_node(rest))
    for i in range(0, len(ssr), 50):
        obs.update(_node(ssr[i:i + 50]))
    obs.update(_py(list(py_jobs.values())))
    from concurrent.futures import ThreadPoolExecutor
    fns = {'asciidoctor_convert': _asciidoctor, 'asciidoctor_equal': _asciidoctor, 'msgfmt_check': _msgfmt,
           'msgfmt_equal': _msgfmt, 'latex_compile': _latex, 'latex_dvi_equal': _latex}
    with ThreadPoolExecutor(8) as pool:
        results = list(pool.map(lambda kv: (kv[0], fns[kv[1][0]](kv[1][1])), local.items()))
    obs.update(dict(results))
    return obs


def _key(c, side, oracle):
    mode = ORACLE_MODE.get(oracle, oracle)
    extra = c['witness_plan'].get('modern') if mode == 'svelte_parse' else \
        c['witness_plan'].get('mode') if mode == 'tinycss2' else None
    return f"{mode}|{c[side]['sha256']}|{extra}"


TOOL_NAMES = {'asciidoctor': 'Asciidoctor (asciidoctor -s, html5)', 'msgfmt': 'GNU msgfmt --check', 'latex': 'pdfTeX latex (DVI)'}


def _tool_versions():
    v = {}
    for name, cmd in (('asciidoctor', [TOOLS['asciidoctor'], '--version']), ('msgfmt', [TOOLS['msgfmt'], '--version']),
                      ('latex', [TOOLS['latex'], '--version'])):
        try:
            v[name] = subprocess.run(cmd, capture_output=True, text=True, timeout=30).stdout.splitlines()[0]
        except (OSError, IndexError, subprocess.TimeoutExpired):
            v[name] = None
    return v


_SPEC_CACHE = {}


def _spec_html(suite, path, number):
    if (suite, path) not in _SPEC_CACHE:
        _SPEC_CACHE[(suite, path)] = {e['number']: e for e in spec_examples(upbytes(suite, path).decode('utf-8'))}
    return _SPEC_CACHE[(suite, path)][number]['html']


_DAT_CACHE = {}


def _dat(path, index):
    if path not in _DAT_CACHE:
        _DAT_CACHE[path] = dat_tests(upbytes('wpt-html-tree-construction', path).decode('utf-8'))
    return _DAT_CACHE[path][index]


def judge_witness(c, obs, versions):
    plan = c['witness_plan']; oracle = plan['oracle']; claim = c['behavior']['claim']
    o = obs.get(_key(c, 'old', oracle)) or {}
    tool = o.get('version') or versions.get({'asciidoctor_convert': 'asciidoctor', 'asciidoctor_equal': 'asciidoctor',
                                             'msgfmt_check': 'msgfmt', 'msgfmt_equal': 'msgfmt', 'latex_compile': 'latex',
                                             'latex_dvi_equal': 'latex'}.get(oracle, ''), oracle)
    tool = f'{oracle}: {tool}'
    if o.get('crash'):
        return _record(c, 'inconclusive', tool, {'crash': o['crash']}, 'reference crashed')
    if c['lane'] == 'parser_fidelity':
        accepts = plan['accepts']; record = {}
        if oracle == 'commonmark_example':
            want = _spec_html('commonmark-spec', 'package/spec.txt', plan['example'])
            if o.get('html') != want:
                return _record(c, 'inconclusive', tool, {'html_equal': False}, 'commonmark.js does not reproduce the spec HTML')
            got = True; record['html_equal'] = True
            record['inventory'] = _blocks_inventory((ROOT / c['old']['path']).read_bytes(), o['blocks'])
        elif oracle == 'gfm_example':
            want = _spec_html('gfm-spec', 'test/spec.txt', plan['example'])
            if o.get('html') != want:
                return _record(c, 'inconclusive', tool, {'html_equal': False}, 'cmark-gfm does not reproduce the spec HTML')
            got = True; record['html_equal'] = True
        elif oracle == 'html5lib_tree':
            t = _dat(plan['file'], plan['index'])
            if o.get('tree') != t['document']:
                return _record(c, 'inconclusive', tool, {'tree_equal': False, 'errors': o.get('errors')},
                               'html5lib does not reproduce the upstream #document')
            got = True; record.update(tree_equal=True, reference_parse_errors=o.get('errors'))
        elif oracle == 'tinycss2_json':
            items = json.loads(upbytes('css-parsing-tests', plan['file']))
            want = items[plan['index'] * 2 + 1]
            if json.dumps(o.get('json')) != json.dumps(want) and o.get('json') != want:
                return _record(c, 'inconclusive', tool, {'json_equal': False}, 'tinycss2 does not reproduce the upstream AST')
            got = True; record['json_equal'] = True
        elif oracle == 'sass_compile':
            got = bool(o.get('ok')); record['error'] = o.get('error')
            if got and accepts:
                want = upbytes('sass-spec', f"{plan['dir']}/expected_output.css").decode('utf-8')
                record['css_equal'] = (o.get('css') or '').strip() == want.strip()
                if not record['css_equal']:
                    return _record(c, 'inconclusive', tool, record, 'Dart Sass output differs from expected_output.css')
            if not got and not accepts and not re.match(r'(expected|Invalid CSS)', o.get('error') or ''):
                return _record(c, 'inconclusive', tool, record, 'Dart Sass rejects, but not with a syntax error')
        elif oracle in ('svelte_parse', 'vue_compile', 'astro_transform', 'mdx_compile'):
            got = bool(o.get('ok')); record = {k: o.get(k) for k in ('error', 'errors', 'diagnostics') if o.get(k)}
        elif oracle in ('asciidoctor_convert', 'msgfmt_check', 'latex_compile'):
            got = bool(o.get('ok')); record = {k: v for k, v in o.items() if k not in ('ok',) and v}
            if oracle == 'latex_compile' and not got and o.get('missing'):
                return _record(c, 'inconclusive', tool, record, 'document needs files absent from the reference TeX installation')
        else:
            raise ValueError(oracle)
        record.update(reference_accepts=got, upstream_accepts=accepts)
        verdict = 'supports_claim' if got == accepts else 'contradicts_claim'
        return _record(c, verdict, tool, record, 'reference and upstream label agree' if got == accepts
                       else 'reference disagrees with the upstream label')
    n = obs.get(_key(c, 'new', oracle)) or {}
    if n.get('crash'):
        return _record(c, 'inconclusive', tool, {'crash': n['crash']}, 'reference crashed on the new side')
    field = {'commonmark_render': 'html', 'sass_equal': 'css', 'svelte_ssr_equal': ('html', 'head'),
             'vue_ssr_equal': 'html', 'astro_transform_equal': 'code_sha256', 'mdx_compile_equal': 'code_sha256',
             'html5lib_equal': 'tree', 'tinycss2_equal': 'canonical', 'asciidoctor_equal': 'html_sha256',
             'msgfmt_equal': 'mo_sha256', 'latex_dvi_equal': 'dvi_sha256'}[oracle]
    fields = field if isinstance(field, tuple) else (field,)
    if any(o.get(f) is None or n.get(f) is None for f in fields[:1]) or o.get('ok') is False or n.get('ok') is False:
        return _record(c, 'inconclusive', tool, {'old': {k: v for k, v in o.items() if k in ('error', 'errors', 'stderr', 'missing')},
                                                 'new': {k: v for k, v in n.items() if k in ('error', 'errors', 'stderr', 'missing')}},
                       'reference could not process both sides')
    obsd = {}
    if oracle == 'html5lib_equal' and c['old']['path'].startswith(DERIVED):
        # old side is an extracted upstream #data: the reference must reproduce the upstream tree first
        f, i = c['upstream']['case_id'].split('#')
        t = _dat(f'html/syntax/parsing/resources/{f}', int(i))
        obsd['old_tree_equals_upstream'] = o['tree'] == t['document']
        if not obsd['old_tree_equals_upstream']:
            return _record(c, 'inconclusive', tool, obsd, 'html5lib does not reproduce the seed\'s upstream #document')
    same = all(o.get(f) == n.get(f) for f in fields)
    obsd['reference_equal'] = same
    if not isinstance(field, tuple) and field.endswith('sha256'):
        obsd['old_' + field], obsd['new_' + field] = o.get(field), n.get(field)
    verdict = 'supports_claim' if same == (claim == 'preserved') else 'contradicts_claim'
    return _record(c, verdict, tool, obsd, f"reference says {'equal' if same else 'different'}; claim {claim}")


def witness(languages):
    cases, _ = define(languages)
    versions = _tool_versions()
    obs = observe(cases)
    tally = Counter()
    for c in cases:
        rec = judge_witness(c, obs, versions)
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
        print(f'markup family build failed: {error}', file=sys.stderr)
        return 1
    if args.check:
        if not FRAGMENT.exists() or FRAGMENT.read_bytes() != data:
            print('markup family fragment is stale; re-run scripts/markup_family.py build', file=sys.stderr)
            return 1
        return 0
    FRAGMENT.parent.mkdir(parents=True, exist_ok=True)
    FRAGMENT.write_bytes(data)
    fragment = json.loads(data)
    tally = Counter((c['language_id'], c['lane'], c['class'], c['admission']['status']) for c in fragment['cases'])
    for key in sorted(tally):
        print(*key, tally[key])
    return 0


if __name__ == '__main__':
    sys.exit(main())
