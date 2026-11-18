#!/usr/bin/env python3
"""Acquire the #263 web, markup and embedded-language external suites at immutable revisions.

Benchmark tooling only (core #263 / #268). Two acquisition routes are used, and both
end in exact retained bytes with their SHA-256 under
``tests/fixtures/external/upstream/markup/<suite_id>/`` plus an ``ACQUISITION.json``:

* ``raw.githubusercontent.com/<owner>/<repo>/<40-hex commit>/<path>``: a fetch at an
  immutable commit, never a branch. The commit is resolved from the tag or branch with
  ``git ls-remote`` (recorded per suite). GitHub tree listings, the REST API and clones
  are not reachable from the benchmark sandbox, so each suite is enumerated from an
  index file committed in the suite itself, or by probing a recalled candidate name
  list; a probed enumeration can only under-enumerate and is recorded as incomplete.
* the npm registry, for suites published as packages that vendor the test data
  (``commonmark-spec``, ``sass-spec``): the tarball's SHA-256 and the registry's SRI
  integrity are recorded together with the upstream ``gitHead`` commit, and only the
  selected member files are retained.

Nothing is substituted for an unreachable or unsuitable source: such a suite is
recorded by ``scripts/markup_family.py`` as a blocked acquisition or a gap.

    python scripts/acquire_markup_benchmarks.py [--suite ID ...]   # network
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures' / 'external' / 'upstream' / 'markup'
RAW = 'https://raw.githubusercontent.com'
NPM = 'https://registry.npmjs.org'
_spec = importlib.util.spec_from_file_location('acquire_structured', Path(__file__).with_name('acquire_structured_benchmarks.py'))
_structured = importlib.util.module_from_spec(_spec); _spec.loader.exec_module(_structured)
fetch = _structured.fetch  # same immutable-URL fetcher (404 -> None, 429 back-off) as #265

# Immutable revisions, resolved with `git ls-remote https://github.com/<repo> <ref>` on
# 2026-11-17 (tags: the peeled commit). raw.githubusercontent.com 404s for a commit that
# does not exist in the repository, so each fetch also confirms the pin.
SUITES = {
    'commonmark-spec': {'repo': 'commonmark/commonmark-spec', 'ref': '0.31.2',
                        'revision': '9103e341a973013013bb1a80e13567007c5cef6f',
                        'npm': ('commonmark-spec', '0.31.2')},
    'gfm-spec': {'repo': 'github/cmark-gfm', 'ref': '0.29.0.gfm.13',
                 'revision': '587a12bb54d95ac37241377e6ddc93ea0e45439b'},
    # html5lib-tests' README at c5495c53 says the tree-construction tests are now solely
    # maintained in WPT html/syntax/parsing; the WPT copy is therefore the upstream.
    'wpt-html-tree-construction': {'repo': 'web-platform-tests/wpt', 'ref': 'master (HEAD)',
                                   'revision': 'edd042b63671f8ba064d86560836171e0cdb3956'},
    'css-parsing-tests': {'repo': 'SimonSapin/css-parsing-tests', 'ref': 'master',
                          'revision': '203ce36bffd617db7f118c551e32794561fb273d'},
    'sass-spec': {'repo': 'sass/sass-spec', 'ref': 'npm sass-spec@3.5.4-1 (gitHead)',
                  'revision': '8be4e1b7555e5560ed1767605ebefc0429756ece', 'npm': ('sass-spec', '3.5.4-1')},
    'svelte-tests': {'repo': 'sveltejs/svelte', 'ref': 'svelte@5.57.3',
                     'revision': '23bb146306ff02cc5041bca0c5de9f8bd086e4a3'},  # peeled annotated tag 87098153,
    'vue-compiler-sfc-tests': {'repo': 'vuejs/core', 'ref': 'v3.5.45',
                               'revision': '8b479a89445868d474f17e2d837765e7c9ece5e4'},
    'astro-compiler-tests': {'repo': 'withastro/compiler', 'ref': '@astrojs/compiler@4.0.0',
                             'revision': '271eee2e499432015ab4c6dc510c5331e1961b84'},
    'mdx-docs': {'repo': 'mdx-js/mdx', 'ref': '3.1.1', 'revision': '50aa8df0b027c893dec9f97a2b7c51539e9f1a4b'},
    'asciidoctor-doctest': {'repo': 'asciidoctor/asciidoctor-doctest', 'ref': 'master',
                            'revision': '1f48884cb953e0315490837fbc30c1558c5752a0'},
    'polib-tests': {'repo': 'izimobil/polib', 'ref': 'master', 'revision': 'bcb2bbab34448ca2e703a364ee523a6241eeff72'},
    'latex2e-base': {'repo': 'latex3/latex2e', 'ref': 'develop (HEAD)',
                     'revision': 'bd4c0b10fb93bd0dfdf2ed1b4660b05ff74fd2ca'},
}

HTML5LIB_TREE = '''adoption01 adoption02 blocks comments01 doctype01 domjs-unsafe entities01 entities02
foreign-fragment html5test-com inbody01 isindex main-element math menuitem-element namespace-sensitivity
noscript01 pending-spec-changes pending-spec-changes-plain-text-unsafe plain-text-unsafe quirks01 ruby
scriptdata01 search-element svg tables01 template tests1 tests2 tests3 tests4 tests5 tests6 tests7 tests8
tests9 tests10 tests11 tests12 tests13 tests14 tests15 tests16 tests17 tests18 tests19 tests20 tests21
tests22 tests23 tests24 tests25 tests26 tests_innerHTML_1 tricky01 webkit01 webkit02 attributes
selectedcontent'''.split()
CSS_FILES = '''An+B blocks_contents color3 color3_hsl color3_keywords color4_hsl color4_hwb color4_keywords
color4_lab_lch_oklab_oklch color4_color_function component_value_list declaration_list one_component_value
one_declaration one_rule rule_list stylesheet stylesheet_bytes urange'''.split()

SVELTE_MODERN = '''attachments comment-before-function-binding comment-before-script comment-in-tag
css-nth-syntax css-pseudo-classes each-block-object-pattern each-block-object-pattern-special-characters
generic-snippets if-block if-block-else if-block-elseif options semicolon-inside-quotes snippets
template-shadowroot typescript-in-event-handler loose-invalid-block loose-invalid-expression
loose-unclosed-open-tag loose-unclosed-tag loose-valid-each-as loose-unclosed-block loose-unclosed-tag-2
script-style-no-markup whitespace-after-script whitespace-after-style'''.split()
SVELTE_LEGACY = '''action action-duplicate action-with-call action-with-identifier action-with-literal animation
attribute-containing-solidus attribute-curly-bracket attribute-dynamic attribute-dynamic-boolean
attribute-escaped attribute-multiple attribute-shorthand attribute-static attribute-static-boolean
attribute-with-whitespace await-catch await-then-catch binding binding-shorthand comment comment-with-ignores
component-dynamic convert-entities convert-entities-in-element css dynamic-element-string dynamic-element-variable
dynamic-import each-block each-block-destructured each-block-else each-block-indexed each-block-keyed
element-with-attribute element-with-attribute-empty-string element-with-mustache element-with-text elements
event-handler generic-snippets if-block if-block-else if-block-elseif implicitly-closed-li
implicitly-closed-li-block javascript-comments nbsp no-error-if-before-closing raw-mustaches refs script
script-attribute-with-curly-braces script-comment-only script-comment-trailing-multiline self-closing-element
self-reference slotted-element space-between-mustaches spread style-inside-head textarea-children
textarea-end-tag transition-intro transition-intro-no-params unusual-identifier whitespace-after-script-tag
whitespace-after-style-tag whitespace-leading-trailing whitespace-normal'''.split()
SVELTE_MIGRATE = '''accessors css-ignore derivations effects effects-with-alias event-handlers
event-handlers-with-alias export-props-multiple-declarations html-comment impossible-migrate-$$props
is-not-where jsdoc-with-comments named-slots not-blank-css-if-error props props-and-labeled
props-export-alias props-interface props-rest-props props-ts reactive-statements-reorder-1
reactive-statements-reorder-2 reactive-statements-reorder-not-deleting-additions remove-blocks-whitespace
self-closing-elements shadowed-forwarded-slot single-assignment-labeled slot-non-identifier slot-shadow-props
slot-use_ts slots slots-below-imports slots-custom-element slots-with-$$props state-and-derivations-sequence
state-no-initial state-ts svelte-component svelte-self svelte-self-name-conflict svelte-self-skip-filename
unused-beforeupdate-afterupdate'''.split()

ASCIIDOC_EXAMPLES = '''block_admonition block_audio block_colist block_dlist block_example block_floating_title
block_image block_listing block_literal block_olist block_open block_outline block_page_break block_paragraph
block_pass block_preamble block_quote block_section block_sidebar block_stem block_table block_thematic_break
block_toc block_ulist block_verse block_video document embedded inline_anchor inline_break inline_button
inline_callout inline_footnote inline_image inline_indexterm inline_kbd inline_menu inline_quoted'''.split()

POLIB_FILES = '''test_utf8.po test_iso-8859-15.po test_fuzzy_header.po test_merge.pot test_merge_after.po
test_merge_before.po test_weird_occurrences.po test_obsolete_previousmsgid.po test_previous_msgid.po
test_wrap.po test_pot_header.pot test_no_header.po test_indented.po test_spaces_inside_quotes.po
test_unescaped_double_quote.po test_save_as_mofile.po test_fuzzy_obsolete.po test_msgctxt.po test_noheader.po
test_obsolete.po test_obsolete_msgctxt.po test_occurrences.po test_utf8_bom.po test_fuzzy.po test_header.po
test_wrapped.po test_ufeff.po test_previous_msgid_msgctxt.po test_iso-8859-15_with_bom.po test_no_header.po
test_unterminated_quote.po test_comment_ordering.po test_merge_unescaped_dquote.po test_merge_expected.po
test_utf8.mo test_iso-8859-15.mo test_save_as_pofile.mo test_msgctxt.mo test_obsolete_msgctxt.mo
test_fuzzy.mo test_save_as_mofile.mo test_utf8_with_bom.po test_missing_quote.po test_newline_after_msgid.po
test_msgid_plural.po test_plural.po test_plural_forms.po test_comments.po test_flags.po'''.split()

MDX_DOCS = '''docs/index.mdx docs/playground.mdx docs/table-of-components.mdx docs/docs/what-is-mdx.mdx
docs/docs/getting-started.mdx docs/docs/using-mdx.mdx docs/docs/extending-mdx.mdx
docs/docs/troubleshooting-mdx.mdx docs/guides/embed.mdx docs/guides/frontmatter.mdx docs/guides/gfm.mdx
docs/guides/math.mdx docs/guides/mdx-on-demand.mdx docs/guides/syntax-highlighting.mdx
docs/guides/injecting-components.mdx docs/community/contribute.mdx docs/community/support.mdx
docs/community/sponsor.mdx docs/community/projects.mdx docs/community/index.mdx docs/blog/index.mdx
docs/blog/v2.mdx docs/blog/v3.mdx docs/blog/shortcodes.mdx docs/blog/custom-pragma.mdx docs/migrating/v2.mdx
docs/migrating/v3.mdx docs/packages/index.mdx docs/docs/index.mdx docs/guides/index.mdx docs/_config.js'''.split()

LATEX_UNUSED = '''base/sample2e.tex base/small2e.tex base/testpage.tex base/lablst.tex base/idx.tex
base/nfssfont.tex base/cfgguide.tex base/clsguide.tex base/fntguide.tex base/usrguide.tex base/encguide.tex
base/modguide.tex base/ltnews.tex base/latexsym.dtx base/docstrip.tex base/lppl.tex base/manifest.txt
base/build.lua base/testfiles/test2e.tex'''.split()


class Acquirer:
    def __init__(self, suite_id):
        self.suite_id = suite_id; self.meta = SUITES[suite_id]
        self.dir = ROOT / suite_id; self.files = {}; self.absent = []; self.notes = []; self.extra = {}

    def keep(self, local, data, record):
        target = self.dir / local
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        self.files[local] = record

    def get(self, path, *, required=True):
        data = fetch(self.meta['repo'], self.meta['revision'], path)
        if data is None:
            if required:
                raise RuntimeError(f'{self.suite_id}: required file missing at pin: {path}')
            self.absent.append(path)
            return None
        url = f"{RAW}/{self.meta['repo']}/{self.meta['revision']}/{path}"
        self.keep(path, data, {'url': url, 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)})
        return data

    def many(self, paths, required=False):
        with ThreadPoolExecutor(6) as pool:
            return dict(zip(paths, pool.map(lambda p: self.get(p, required=required), paths)))

    def npm(self):
        name, version = self.meta['npm']
        meta = json.loads(subprocess.run(['curl', '-sS', '--max-time', '60', f'{NPM}/{name}/{version}'],
                                         capture_output=True, check=True).stdout)
        tarball = subprocess.run(['curl', '-sS', '--max-time', '120', meta['dist']['tarball']],
                                 capture_output=True, check=True).stdout
        if meta.get('gitHead') != self.meta['revision']:
            raise RuntimeError(f'{self.suite_id}: npm gitHead {meta.get("gitHead")} != pinned revision')
        self.extra['npm'] = {'package': name, 'version': version, 'tarball': meta['dist']['tarball'],
                             'tarball_sha256': hashlib.sha256(tarball).hexdigest(), 'tarball_bytes': len(tarball),
                             'integrity': meta['dist'].get('integrity'), 'gitHead': meta.get('gitHead'),
                             'licence': meta.get('license')}
        return tarfile.open(fileobj=io.BytesIO(tarball), mode='r:gz'), meta['dist']['tarball']

    def member(self, tar, url, name):
        data = tar.extractfile(name).read()
        self.keep(name, data, {'url': f'{url}#{name}', 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)})
        return data

    def finish(self, enumeration):
        manifest = {'schema_version': 1, 'suite_id': self.suite_id, 'repository': self.meta['repo'],
                    'ref': self.meta['ref'], 'revision': self.meta['revision'],
                    'revision_resolution': f"git ls-remote https://github.com/{self.meta['repo']} for {self.meta['ref']} "
                                           '(2026-11-17); bytes fetched only at that commit'
                                           + (' or from the npm tarball whose gitHead is that commit' if 'npm' in self.meta else ''),
                    'enumeration': enumeration, 'files': dict(sorted(self.files.items())),
                    'probed_absent': sorted(set(self.absent)), 'notes': self.notes, **self.extra}
        (self.dir / 'ACQUISITION.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
        print(f'{self.suite_id}: {len(self.files)} files, {len(set(self.absent))} probed-absent')


def acquire_commonmark():
    a = Acquirer('commonmark-spec')
    tar, url = a.npm()
    npm_spec = a.member(tar, url, 'package/spec.txt'); a.member(tar, url, 'package/LICENSE')
    a.member(tar, url, 'package/package.json')
    raw_spec = fetch(a.meta['repo'], a.meta['revision'], 'spec.txt')
    a.extra['raw_cross_check'] = {'path': 'spec.txt', 'sha256': hashlib.sha256(raw_spec).hexdigest(),
                                  'equals_npm_member': raw_spec == npm_spec}
    if raw_spec != npm_spec:
        raise RuntimeError('commonmark-spec: npm spec.txt differs from the pinned commit')
    a.notes.append('package/spec.txt is byte-identical to spec.txt at the pinned commit (raw.githubusercontent.com).')
    a.finish({'method': 'every example in spec.txt (the spec\'s own example fences)', 'complete': True})


def acquire_gfm():
    a = Acquirer('gfm-spec')
    a.get('COPYING'); a.get('test/spec.txt')
    a.notes.append('COPYING covers cmark-gfm code (BSD-2-Clause); the spec text itself carries the CC-BY-SA 4.0 '
                   'notice in its front matter.')
    a.finish({'method': 'every example in test/spec.txt', 'complete': True})


def acquire_html5lib():
    a = Acquirer('wpt-html-tree-construction')
    a.get('LICENSE.md')
    got = a.many([f'html/syntax/parsing/resources/{n}.dat' for n in HTML5LIB_TREE])
    a.notes.append('html5lib/html5lib-tests@c5495c539ca8a79c3c5107f5f14010bcff9a35d5 README.md: "The HTML parser tree '
                   'construction tests are now solely maintained on web-platform-tests" (html/syntax/parsing); its '
                   'tree-construction/ directory no longer exists at that commit, so WPT is the upstream used.')
    a.finish({'method': 'probe a recalled candidate file list (no in-suite index)', 'complete': False,
              'candidates': len(HTML5LIB_TREE), 'found': sorted(k for k, v in got.items() if v is not None)})


def acquire_css():
    a = Acquirer('css-parsing-tests')
    a.get('LICENSE'); a.get('README.rst')
    got = a.many([f'{n}.json' for n in CSS_FILES])
    a.finish({'method': 'file names documented in README.rst, probed', 'complete': False,
              'found': sorted(k for k, v in got.items() if v is not None)})


SASS_DIRS = ('spec/basic/', 'spec/scss/', 'spec/css/', 'spec/errors/')


def acquire_sass():
    a = Acquirer('sass-spec')
    tar, url = a.npm()
    a.member(tar, url, 'package/LICENSE'); a.member(tar, url, 'package/package.json')
    names = sorted(m.name for m in tar.getmembers() if m.isfile())
    dirs = sorted({n.rsplit('/', 1)[0] for n in names if n.endswith('/input.scss')
                   and any(n.startswith('package/' + d) for d in SASS_DIRS)})
    sizes = {m.name: m.size for m in tar.getmembers() if m.isfile()}
    large = [d for d in dirs if sizes.get(f'{d}/input.scss', 0) > 65536]
    dirs = [d for d in dirs if d not in large]
    a.extra['excluded_large_inputs'] = large
    for d in dirs:
        for f in ('input.scss', 'expected_output.css', 'error', 'options.yml', 'status'):
            if f'{d}/{f}' in names:
                a.member(tar, url, f'{d}/{f}')
    a.extra['tarball_input_scss_total'] = sum(n.endswith('/input.scss') for n in names)
    a.notes.append('Retained: every test directory under ' + ', '.join(SASS_DIRS) + ' (input.scss with its '
                   'expected_output.css / error / options.yml / status). Other directories are discovered, not imported.')
    a.finish({'method': 'npm tarball member listing (complete for the package)', 'complete': True,
              'selected_dirs': len(dirs)})


def acquire_svelte():
    a = Acquirer('svelte-tests')
    a.get('LICENSE.md')
    base = 'packages/svelte/tests'
    modern = a.many([f'{base}/parser-modern/samples/{n}/input.svelte' for n in SVELTE_MODERN])
    found_modern = [n for n in SVELTE_MODERN if modern[f'{base}/parser-modern/samples/{n}/input.svelte'] is not None]
    a.many([f'{base}/parser-modern/samples/{n}/output.json' for n in found_modern])
    legacy = a.many([f'{base}/parser-legacy/samples/{n}/input.svelte' for n in SVELTE_LEGACY])
    found_legacy = [n for n in SVELTE_LEGACY if legacy[f'{base}/parser-legacy/samples/{n}/input.svelte'] is not None]
    a.many([f'{base}/parser-legacy/samples/{n}/output.json' for n in found_legacy])
    migrate = a.many([f'{base}/migrate/samples/{n}/input.svelte' for n in SVELTE_MIGRATE])
    found_migrate = [n for n in SVELTE_MIGRATE if migrate[f'{base}/migrate/samples/{n}/input.svelte'] is not None]
    a.many([f'{base}/migrate/samples/{n}/{f}' for n in found_migrate for f in ('output.svelte', '_config.js')])
    a.finish({'method': 'probe recalled sample directory names (no in-suite index)', 'complete': False,
              'parser_modern_found': found_modern, 'parser_legacy_found': found_legacy,
              'migrate_found': found_migrate,
              'candidates': len(SVELTE_MODERN) + len(SVELTE_LEGACY) + len(SVELTE_MIGRATE)})


def acquire_vue():
    a = Acquirer('vue-compiler-sfc-tests')
    a.get('LICENSE')
    a.get('packages/compiler-sfc/__tests__/parse.spec.ts')
    a.get('packages/compiler-sfc/__tests__/compileScript.spec.ts')
    a.finish({'method': 'fixed upstream test modules; SFC sources are extracted from their template literals',
              'complete': True})


def acquire_astro():
    a = Acquirer('astro-compiler-tests')
    a.get('LICENSE')
    a.get('internal/printer/printer_test.go')
    a.finish({'method': 'fixed upstream test module; Astro sources are extracted from its `source:` raw strings',
              'complete': True})


def acquire_mdx():
    a = Acquirer('mdx-docs')
    a.get('license')
    got = a.many(MDX_DOCS)
    a.finish({'method': 'probe recalled documentation paths (no in-suite index)', 'complete': False,
              'found': sorted(k for k, v in got.items() if v is not None)})


def acquire_asciidoc():
    a = Acquirer('asciidoctor-doctest')
    a.get('LICENSE')
    got = a.many([f'data/examples/asciidoc/{n}.adoc' for n in ASCIIDOC_EXAMPLES])
    a.finish({'method': 'probe recalled example file names (no in-suite index)', 'complete': False,
              'found': sorted(k for k, v in got.items() if v is not None)})


def acquire_polib():
    a = Acquirer('polib-tests')
    a.get('LICENSE')
    tests = a.get('tests/tests.py', required=False) or b''
    named = sorted(set(re.findall(rb"tests/([A-Za-z0-9_.-]+\.(?:po|pot))", tests)))
    names = sorted(set(n.decode() for n in named) | {n for n in POLIB_FILES if n.endswith(('.po', '.pot'))})
    got = a.many([f'tests/{n}' for n in names])
    a.finish({'method': '.po/.pot files named in tests/tests.py, plus probed recalled names', 'complete': False,
              'named_in_tests_py': [n.decode() for n in named],
              'found': sorted(k for k, v in got.items() if v is not None)})


def acquire_latex():
    a = Acquirer('latex2e-base')
    a.get('LICENSE')
    manifest = a.get('base/manifest.txt').decode()
    names = sorted(set(re.findall(r'^% ([A-Za-z0-9-]+\.tex)\s+--', manifest, re.M)))
    found = []
    for name in names:  # manifest names files, not their directory: try base/ then base/doc/
        if a.get(f'base/{name}', required=False) is not None or a.get(f'base/doc/{name}', required=False) is not None:
            found.append(name)
    a.notes.append('Enumeration: the .tex documents listed in base/manifest.txt (the distribution\'s own manifest), '
                   'probed at base/ and base/doc/. The l3build regression files (base/testfiles/*.lvt) are not listed '
                   'in any retained index and need l3build; they are not imported.')
    a.finish({'method': 'base/manifest.txt .tex entries, probed at base/ and base/doc/', 'complete': True,
              'listed': names, 'found': found})


ACQUIRE = {'commonmark-spec': acquire_commonmark, 'gfm-spec': acquire_gfm, 'wpt-html-tree-construction': acquire_html5lib,
           'css-parsing-tests': acquire_css, 'sass-spec': acquire_sass, 'svelte-tests': acquire_svelte,
           'vue-compiler-sfc-tests': acquire_vue, 'astro-compiler-tests': acquire_astro, 'mdx-docs': acquire_mdx,
           'asciidoctor-doctest': acquire_asciidoc, 'polib-tests': acquire_polib, 'latex2e-base': acquire_latex}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--suite', nargs='+', choices=sorted(ACQUIRE))
    args = parser.parse_args(argv)
    for suite in args.suite or sorted(ACQUIRE):
        ACQUIRE[suite]()
    return 0


if __name__ == '__main__':
    sys.exit(main())
