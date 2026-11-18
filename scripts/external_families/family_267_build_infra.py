"""#267 build, configuration and infrastructure family for the schema-2 admission ledger (core #268).

Benchmark tooling only. Contributes suites, discovery rows, cases and blocked acquisitions for the
registered IDs dockerfile, hcl, gomod, gitignore, cmake, make and puppet. ``bash`` and ``powershell``
are scripting languages and are left to #266.

Upstream bytes are vendored verbatim under ``tests/fixtures/external/upstream/build-infra/<suite>/``
(``scripts/acquire_build_infra_benchmarks.py``). Derived and extracted bytes go under
``admission/cases/<language>/``. Admission is decided only by the executed witness records under
``admission/witnesses/build-infra/`` (written by ``scripts/build_infra_witness.py``, which runs the
reference tools outside IntentumDiff) plus the source review recorded here. IntentumDiff output is
never consulted.

``plan(root, write)`` is shared with the witness script: it lists every case with its old/new bytes,
its behaviour claim and the witness plan. ``contribute(ctx)`` turns the plan plus the witness records
into ledger cases.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re

TOOL = 'scripts/external_families/family_267_build_infra.py'
UP = 'upstream/build-infra'
WITNESSES = 'admission/witnesses/build-infra'
ALL_TYPES = ('ADDITION', 'DELETION', 'MODIFICATION', 'REFACTORING', 'MOVE', 'REORDER')
CODE_TYPES = ('ADDITION', 'DELETION', 'MODIFICATION', 'MOVE', 'REORDER')
EDIT_TYPES = ['ADDITION', 'DELETION', 'MODIFICATION', 'MOVE']

WITNESS_NOTE = ('Executed by scripts/build_infra_witness.py with the reference tool named in the witness record, '
                'against the exact case bytes. Reference outcomes are bounded evidence for the exercised inputs and '
                'the stated observation domain. IntentumDiff output is not an input to this oracle.')

SUITES = {
    'buildkit-dockerfile-parser': dict(
        language='dockerfile', name='BuildKit Dockerfile parser test files (frontend/dockerfile/parser)',
        version='v0.17.3', url='https://github.com/moby/buildkit/tree/v0.17.3/frontend/dockerfile/parser',
        revision='dc30e8355a4691bb34a23dd7161306977eecd5d8', licence='Apache-2.0',
        licence_evidence='LICENSE and NOTICE in the verified module zip, retained',
        attribution='BuildKit, Moby project authors; Apache-2.0 (LICENSE and NOTICE retained).',
        evidence_class='official upstream regression suite',
        dialect='Dockerfile, BuildKit v0.17.3 parser (default escape token \\)'),
    'hcl-specsuite': dict(
        language='hcl', name='HCL specification test suite (specsuite/tests)', version='v2.23.0',
        url='https://github.com/hashicorp/hcl/tree/v2.23.0/specsuite', revision='56a9aee5207dbaed7f061cd926b96fc159d26ea0',
        licence='MPL-2.0', licence_evidence='LICENSE in the verified module zip, retained',
        attribution='HCL, HashiCorp, Inc.; MPL-2.0 (LICENSE retained). Files are unmodified.',
        evidence_class='standards/conformance suite', dialect='HCL native syntax (hclsyntax v2.23.0)'),
    'terraform-configs-testdata': dict(
        language='hcl', name='Terraform configuration loader test files (internal/configs/testdata)', version='v1.5.7',
        url='https://github.com/hashicorp/terraform/tree/v1.5.7/internal/configs/testdata',
        revision='ee58ac1851c8a433005df9863ed47796a9f6b5e7', licence='MPL-2.0',
        licence_evidence='LICENSE (Mozilla Public License 2.0) in the verified v1.5.7 module zip, retained; v1.5.7 '
                         'predates the BUSL-1.1 relicensing in v1.6',
        attribution='Terraform, HashiCorp, Inc.; MPL-2.0 (LICENSE retained). Files are unmodified.',
        evidence_class='official upstream regression suite',
        dialect='Terraform language files in HCL native syntax (hclsyntax v2.23.0)'),
    'x-mod-modfile': dict(
        language='gomod', name='golang.org/x/mod modfile test data (.in and .golden)', version='v0.22.0',
        url='https://cs.opensource.google/go/x/mod/+/refs/tags/v0.22.0:modfile/testdata',
        revision='dec0365065b75edd0e98b0306f6f9b0051710ed2', licence='BSD-3-Clause',
        licence_evidence='LICENSE in the verified module zip, retained',
        attribution='golang.org/x/mod, The Go Authors; BSD-3-Clause (LICENSE retained).',
        evidence_class='official upstream regression suite',
        dialect='go.mod (x/mod v0.22.0, vendored by go1.24.7; cmd/go go1.24.7)'),
    'go-cmd-mod-testdata': dict(
        language='gomod', name='Go cmd/go module test data (src/cmd/go/testdata/mod) and mod_replace script',
        version='go1.24.7', url='https://github.com/golang/go/tree/go1.24.7/src/cmd/go/testdata/mod',
        revision='d41a8222133aabaf5527eab64586050a0fbb773b', licence='BSD-3-Clause',
        licence_evidence='LICENSE at the pinned commit, retained',
        attribution='Go, The Go Authors; BSD-3-Clause (LICENSE retained).',
        evidence_class='official upstream regression suite', dialect='go.mod (cmd/go go1.24.7)'),
    'git-t0008-ignores': dict(
        language='gitignore', name='Git t0008-ignores.sh', version='v2.43.0',
        url='https://github.com/git/git/blob/v2.43.0/t/t0008-ignores.sh',
        revision='564d0252ca632e0264ed670534a51d18a689ef5d', licence='GPL-2.0-only',
        licence_evidence='COPYING at the pinned commit, retained. The script is vendored as unmodified test data and '
                         'is never linked into or distributed with IntentumDiff binaries; the extracted ignore files '
                         'are here-document payloads of the script and remain GPL-2.0 with the same attribution.',
        attribution='Git test suite t0008-ignores.sh, the Git project contributors; GPL-2.0 (COPYING retained).',
        evidence_class='official upstream regression suite', dialect='gitignore (git 2.43.0 check-ignore)'),
    'cmake-runcmake-syntax': dict(
        language='cmake', name='CMake RunCMake Syntax tests (Tests/RunCMake/Syntax)', version='v3.28.3',
        url='https://github.com/Kitware/CMake/tree/v3.28.3/Tests/RunCMake/Syntax',
        revision='5e984bb35232116a54de7db39788cb162ca7c263', licence='BSD-3-Clause',
        licence_evidence='Copyright.txt at the pinned commit (OSI-approved BSD 3-Clause), retained',
        attribution='CMake, Kitware, Inc. and contributors; BSD-3-Clause (Copyright.txt retained).',
        evidence_class='official upstream regression suite',
        dialect='CMake language, cmake 3.28.3 (RunCMake harness, Unix Makefiles, project(... NONE))'),
    'gnu-make-tests': dict(
        language='make', name='GNU Make regression tests (tests/scripts)', version='4.3',
        url='https://github.com/mirror/make/tree/4.3/tests', revision='f430a65ccb5e85eb465f6a64afcbdd09f9c708ed',
        licence='GPL-3.0-or-later',
        licence_evidence='COPYING at the pinned commit and the GPL-3.0-or-later notice in each test script, '
                         'retained. The scripts and the makefiles their harness writes are vendored as unmodified test '
                         'data with attribution and are never linked into IntentumDiff binaries.',
        attribution='GNU Make test suite, Free Software Foundation, Inc.; GPL-3.0-or-later (COPYING retained).',
        evidence_class='official upstream regression suite', dialect='GNU Make 4.3'),
    'puppet-parser-specs': dict(
        language='puppet', name='Puppet parser specs (spec/unit/pops/parser)', version='main @ e227c2754097 (=8.10.0 for these files)',
        url='https://github.com/puppetlabs/puppet/tree/main/spec/unit/pops/parser',
        revision='e227c27540975c25aa22d533a52424a9d2fc886a', licence='Apache-2.0',
        licence_evidence='LICENSE at the pinned commit, retained', evidence_class='official upstream regression suite',
        attribution='Puppet, Puppet, Inc. and contributors; Apache-2.0 (LICENSE retained).',
        dialect='Puppet language (puppet 8.10.0 gem, Ruby 3.3)'),
}

BLOCKED = [
    {'suite_id': 'iac-eval', 'language_id': 'hcl', 'lanes': ['change_understanding'],
     'name': 'IaC-Eval, first edition (NeurIPS 2024), autoiac-project/iac-eval',
     'url': 'https://huggingface.co/datasets/autoiac-project/iac-eval',
     'reason': 'BLOCKED: huggingface.co refuses the HTTPS CONNECT tunnel (403, 2026-11-17), so no revision-pinned '
               'bytes could be fetched or hashed; the dataset card (via the Hugging Face MCP) declares CC-BY-4.0, '
               'which would permit reuse. Its AWS references would also need registry.terraform.io providers. HCL '
               'change cases come from the HCL spec suite and Terraform testdata instead; IaC-Eval itself is not '
               'claimed.'},
    {'suite_id': 'shipwright', 'language_id': 'dockerfile', 'lanes': ['change_understanding'],
     'name': 'Shipwright (ICSE 2021) Dockerfile repair artifact, STAR-RG/shipwright',
     'url': 'https://github.com/STAR-RG/shipwright',
     'reason': 'BLOCKED: no immutable commit could be resolved (git ls-remote was refused by the sandbox permission '
               'policy; the GitHub API and github.com are unreachable; proxy.golang.org returns not found for the '
               'repository), and its build oracle needs a Docker daemon, which is not available. LICENSE.md (MIT) '
               'was read at branch master only. No Dockerfile was vendored; Dockerfile change cases are derived from '
               'BuildKit test files instead.'},
]

DOCKER_EDITS = [
    ('mail', 'comment', [('RUN apt-get update', '# install the mail tools\nRUN apt-get update')], 'preserved', 'comment',
     'comment line inserted before an instruction'),
    ('mail', 'instruction-case', [('ADD vim /.vim', 'add vim /.vim')], 'preserved', 'style',
     'one instruction keyword lower-cased (instructions are case-insensitive)'),
    ('mail', 'blank-line', [('FROM ubuntu:14.04\n\n', 'FROM ubuntu:14.04\n')], 'preserved', 'style',
     'blank line after FROM removed'),
    ('mail', 'run-argument', [('RUN mkdir /Mail\n', 'RUN mkdir /Maildir\n')], 'changed', 'changed',
     'RUN command argument changed'),
    ('mail', 'cmd-exec-form', [('CMD setsid cron; tmux -2', 'CMD ["setsid", "cron"]')], 'changed', 'changed',
     'CMD shell form replaced by an exec form with different arguments'),
    ('mail', 'base-tag', [('FROM ubuntu:14.04', 'FROM ubuntu:16.04')], 'changed', 'changed', 'base image tag changed'),
    ('nginx', 'expose-port', [('EXPOSE 80', 'EXPOSE 8080')], 'changed', 'changed', 'exposed port changed'),
    ('nginx', 'json-spacing', [('CMD ["/usr/sbin/nginx"]', 'CMD [ "/usr/sbin/nginx" ]')], 'preserved', 'style',
     'whitespace inside the exec-form JSON array'),
    ('nginx', 'add-instruction', [('VOLUME /www\n', 'VOLUME /www\nUSER www-data\n')], 'changed', 'changed',
     'USER instruction added'),
    ('nginx', 'remove-instruction', [('RUN /usr/sbin/nginx -qt\n', '')], 'changed', 'changed', 'RUN instruction removed'),
    ('env', 'env-equals-form', [('ENV name value\n', 'ENV name=value\n')], 'preserved', 'equivalent',
     'legacy `ENV key value` respelt as `ENV key=value`'),
    ('env', 'env-value', [('ENV name=value name2=value2\n', 'ENV name=value name2=other\n')], 'changed', 'changed',
     'one ENV value changed'),
    ('kartar-entrypoint-oddities', 'env-equals-form', [('ENV REFRESHED_AT 2014-06-01', 'ENV REFRESHED_AT=2014-06-01')],
     'preserved', 'equivalent', 'legacy `ENV key value` respelt as `ENV key=value`'),
    ('kartar-entrypoint-oddities', 'json-spacing', [('ENTRYPOINT [ "/usr/bin/redis-server" ]',
                                                      'ENTRYPOINT ["/usr/bin/redis-server"]')],
     'preserved', 'style', 'whitespace inside the exec-form JSON array'),
    ('kartar-entrypoint-oddities', 'entrypoint-shell-form', [('ENTRYPOINT [ "/usr/bin/redis-server" ]',
                                                               'ENTRYPOINT /usr/bin/redis-server')],
     'changed', 'changed', 'ENTRYPOINT exec form replaced by shell form (runs under /bin/sh -c)'),
    ('health', 'healthcheck-interval', [('--interval=5s --timeout=3s --retries=3', '--interval=10s --timeout=3s --retries=3')],
     'changed', 'changed', 'HEALTHCHECK interval changed'),
]

TF = 'terraform-configs-testdata/internal/configs/testdata/valid-files'
HS = 'hcl-specsuite/specsuite/tests'
HCL_EDITS = [
    (TF + '/outputs.tf', 'comment', [('output "bar" {', '# derived from local.bar\noutput "bar" {')], 'preserved', 'comment',
     'line comment added before a block'),
    (TF + '/outputs.tf', 'value', [('value = "hello"', 'value = "goodbye"')], 'changed', 'changed', 'string value changed'),
    (TF + '/outputs.tf', 'sensitive', [('  sensitive = true\n', '  sensitive = false\n')], 'changed', 'changed',
     'bool attribute changed'),
    (TF + '/outputs.tf', 'label', [('output "foo" {', 'output "foo2" {')], 'changed', 'changed', 'block label renamed'),
    (TF + '/outputs.tf', 'attribute-order', [('  value     = "ssshhhhhhh"\n  sensitive = true\n',
                                               '  sensitive = true\n  value     = "ssshhhhhhh"\n')],
     'preserved', 'reorder', 'two attributes of one block swapped (HCL attributes are an unordered map)'),
    (TF + '/locals.tf', 'alignment', [('  baz    = "oink"\n  dunno  = "🤷"\n', '  baz = "oink"\n  dunno = "🤷"\n')],
     'preserved', 'style', 'attribute alignment whitespace removed'),
    (TF + '/locals.tf', 'remove-attribute', [('  bar = true\n', '')], 'changed', 'changed', 'attribute removed'),
    (TF + '/locals.tf', 'number', [('π      = 3.14159265359', 'π      = 3.14')], 'changed', 'changed', 'number changed'),
    (TF + '/variables.tf', 'object-key-order', [('    "a" = 1,\n    "b" = 2\n', '    "b" = 2,\n    "a" = 1\n')],
     'preserved', 'reorder', 'keys of an object constructor swapped (same object value)'),
    (TF + '/variables.tf', 'object-value', [('    "a" = 1,\n', '    "a" = 10,\n')], 'changed', 'changed',
     'object member value changed'),
    (TF + '/variables.tf', 'block-remove', [('variable "cheeze_pizza" {\n  description = "Nothing special"\n}\n\n', '')],
     'changed', 'changed', 'block removed'),
    (HS + '/expressions/primitive_literals.hcl', 'comment', [('# Booleans\n', '# Booleans (both spellings)\n')],
     'preserved', 'comment', 'comment text changed'),
    (HS + '/expressions/primitive_literals.hcl', 'alignment', [('true  = true\nfalse = false\n', 'true = true\nfalse = false\n')],
     'preserved', 'style', 'alignment whitespace removed'),
    (HS + '/expressions/primitive_literals.hcl', 'string', [('string_ascii           = "hello"', 'string_ascii           = "Hello"')],
     'changed', 'changed', 'string value changed'),
    (HS + '/expressions/primitive_literals.hcl', 'bool', [('false = false', 'false = true')], 'changed', 'changed',
     'bool value changed'),
    (HS + '/structure/attributes/expected.hcl', 'attribute-order', [('a = "a value"\nb = "b value"\n', 'b = "b value"\na = "a value"\n')],
     'preserved', 'reorder', 'two top-level attributes swapped (unordered map)'),
]
HCL_FMT_SEEDS = [TF + '/outputs.tf', TF + '/locals.tf', TF + '/variables.tf', TF + '/resources.tf',
                 HS + '/expressions/primitive_literals.hcl', HS + '/expressions/operators.hcl']

XM = 'x-mod-modfile/modfile/testdata'
GOMOD_REPLACE = 'module quoter\n\nrequire rsc.io/quote/v3 v3.0.0\n\n'  # mod_replace.txt `-- go.mod --`
GOMOD_EDITS = [
    ('mod_replace', 'replace-added', [(GOMOD_REPLACE, GOMOD_REPLACE + 'replace rsc.io/quote/v3 => ./local/rsc.io/quote/v3\n')],
     'changed', 'changed', 'replace directive added (the edit the upstream script makes with go mod edit -replace)'),
    ('mod_replace', 'require-version', [('rsc.io/quote/v3 v3.0.0', 'rsc.io/quote/v3 v3.1.0')], 'changed', 'changed',
     'required version changed'),
    ('mod_replace', 'require-block', [('require rsc.io/quote/v3 v3.0.0\n', 'require (\n\trsc.io/quote/v3 v3.0.0\n)\n')],
     'preserved', 'equivalent', 'single require line regrouped into a require block'),
    ('mod_replace', 'comment', [('module quoter\n', '// quoter prints a quotation.\nmodule quoter\n')], 'preserved',
     'comment', 'plain comment added'),
    ('mod_replace', 'indirect', [('rsc.io/quote/v3 v3.0.0\n', 'rsc.io/quote/v3 v3.0.0 // indirect\n')], 'changed', 'changed',
     '`// indirect` annotation added (cmd/go reports Indirect: true; not an ordinary comment)'),
    ('mod_replace', 'go-directive', [('module quoter\n', 'module quoter\n\ngo 1.22\n')], 'changed', 'changed',
     'go directive added'),
    ('mod_replace', 'exclude', [(GOMOD_REPLACE, GOMOD_REPLACE + 'exclude rsc.io/quote/v3 v3.0.1\n')], 'changed', 'changed',
     'exclude directive added'),
    ('mod_replace', 'spacing', [('require rsc.io/quote/v3 v3.0.0', 'require   rsc.io/quote/v3   v3.0.0')], 'preserved',
     'style', 'whitespace between tokens'),
    (XM + '/replace2.in', 'replace-target', [('xyz v1.5.6 => my/xyz v1.5.6', 'xyz v1.5.6 => my/xyz v1.5.7')], 'changed',
     'changed', 'replacement version changed'),
    (XM + '/replace2.in', 'quoting', [('"xyz" v1.2.3 => "/tmp/z"', 'xyz v1.2.3 => /tmp/z')], 'preserved', 'equivalent',
     'quoted module path and replacement path unquoted (same strings)'),
    (XM + '/replace2.in', 'replace-remove', [('\txyz => my/other/xyz v1.5.4\n', '')], 'changed', 'changed',
     'wildcard replace removed'),
]

T0008 = 'git-t0008-ignores/t/t0008-ignores.sh'
SETUP_UNIVERSE = ['one', 'not-ignored', 'a/one', 'a/not-ignored', 'a/b/on', 'a/b/one', 'a/b/one one', 'a/b/one two',
                  'a/b/not-ignored', 'a/b/two', 'a/b/twooo', 'globaltwo', 'a/globaltwo', 'a/b/globaltwo', 'b/globaltwo',
                  'ignored-x', 'ignored-and-untracked', 'a/ignored-and-untracked', 'top-level-dir/x', 'a/top-level-dir',
                  'a/b/four', 'a/b/five', 'a/b/six', 'a/b/ignored-dir/x', 'a/three', 'a/b/xthree', 'per-repo',
                  'a/per-repo', 'globalone', 'globalthree', 'two', 'b/two3']
DATA_UNIVERSE = ['file', 'data/file', 'data/data1/file1', 'data/data1/file1.txt', 'data/data2/file2',
                 'data/data2/file2.txt']
GIT_EDITS = [
    ('.gitignore', 'comment', [('one\n', '# top-level patterns\none\n')], 'preserved', 'comment', 'comment line added'),
    ('.gitignore', 'anchor', [('ignored-*\n', '/ignored-*\n')], 'changed', 'changed',
     'pattern anchored to the top level (a/ignored-and-untracked is no longer ignored)'),
    ('.gitignore', 'dir-only', [('top-level-dir/\n', 'top-level-dir\n')], 'changed', 'changed',
     'directory-only slash removed (the file a/top-level-dir becomes ignored)'),
    ('.gitignore', 'trailing-space', [('one\n', 'one   \n')], 'preserved', 'style',
     'unescaped trailing spaces (git strips them)'),
    ('.gitignore', 'disjoint-reorder', [('one\nignored-*\n', 'ignored-*\none\n')], 'preserved', 'reorder',
     'two positive patterns with disjoint matches swapped'),
    ('a/b/.gitignore', 'comments-removed', [('# this comment should affect the line numbers\n', ''),
                                             ('# and so should this blank line:\n\n', '')],
     'preserved', 'comment', 'comment lines and the blank line removed'),
    ('a/b/.gitignore', 'negation-reorder', [('!on*\n!two\n', '!two\n!on*\n')], 'preserved', 'reorder',
     'two negations swapped'),
    ('a/.gitignore', 'negation-added', [('*three\n', '*three\n!twooo\n')], 'changed', 'changed',
     'negation added (a/b/twooo is re-included)'),
    ('a/.gitignore', 'glob-removed', [('two*\n', 'two\n')], 'changed', 'changed',
     'trailing glob removed (a/b/twooo is no longer ignored)'),
    ('.git/info/exclude', 'dir-only-added', [('per-repo\n', 'per-repo/\n')], 'changed', 'changed',
     'pattern made directory-only (the files per-repo and a/per-repo are no longer ignored)'),
    ('data/.gitignore', 'dir-negation-removed', [('!data/**/\n', '')], 'changed', 'changed',
     'directory re-inclusion removed, so *.txt below excluded directories cannot be re-included'),
    ('data/.gitignore', 'comment', [('data/**\n', '# data tree\ndata/**\n')], 'preserved', 'comment', 'comment line added'),
]

CMAKE_EDITS = [
    ('CommandSpaces', 'argument-spacing', [('message( "Example Message" )\n', 'message("Example Message")\n')],
     'preserved', 'style', 'whitespace inside a command invocation'),
    ('CommandSpaces', 'comment', [('message ("Example Message")\n', '# second form\nmessage ("Example Message")\n')],
     'preserved', 'comment', 'line comment added'),
    ('CommandSpaces', 'bracket-comment', [('message("Example Message")\n', '#[[ first form ]]\nmessage("Example Message")\n')],
     'preserved', 'comment', 'bracket comment added'),
    ('CommandSpaces', 'argument-value', [('message( "Example Message")\n', 'message( "Example message")\n')],
     'changed', 'changed', 'quoted argument text changed'),
    ('CommandSpaces', 'argument-split', [('message ( Example " " Message )', 'message ( Example "  " Message )')],
     'changed', 'changed', 'quoted space argument changed'),
    ('CommandSpaces', 'command-case', [('  message  ( "Example Message")', '  MESSAGE  ( "Example Message")')],
     'preserved', 'style', 'command name case changed (command names are case-insensitive)'),
    ('String1', 'string-text', [(' 1 \\${var} 4', ' 1 \\${var} 5')], 'changed', 'changed', 'quoted string content changed'),
    ('Bracket1', 'bracket-to-quoted', [('message([==[1 ]==] [=[', 'message("1 " [=[')], 'preserved', 'equivalent',
     'bracket argument respelt as a quoted argument with the same value'),
    ('CommandComments', 'comment-text', [('# "Commented" )', '# "Commented out" )')], 'preserved', 'comment',
     'comment text changed'),
    ('CommandComments', 'argument-added', [('"Second Line of Example")', '"Second Line of Example" " (2)")')],
     'changed', 'changed', 'argument added'),
]

MAKE_EDITS = [
    # (script, test index, edits, claim, kind, description)
    ('variables/flavors', 0, [('foo = $(bar)', 'foo := $(bar)')], 'changed', 'changed',
     'recursive assignment made simple (immediate expansion)'),
    ('variables/flavors', 0, [('foo = $(bar)', 'foo=$(bar)')], 'preserved', 'style', 'whitespace around = removed'),
    ('variables/flavors', 0, [('ugh = Hello\n', '# final value\nugh = Hello\n')], 'preserved', 'comment', 'comment line added'),
    ('variables/flavors', 0, [('ugh = Hello\n', 'ugh = Howdy\n')], 'changed', 'changed', 'variable value changed'),
    ('variables/flavors', 1, [('foo := $(bar)', 'foo = $(bar)')], 'changed', 'changed',
     'simple assignment made recursive (deferred expansion)'),
    ('variables/flavors', 1, [('foo := $(bar)', 'foo ::= $(bar)')], 'preserved', 'equivalent',
     '`:=` respelt as the POSIX `::=` (same simple flavor in GNU Make)'),
    ('variables/flavors', 2, [('foo += $(bar)', 'foo  +=  $(bar)')], 'preserved', 'style', 'whitespace around += widened'),
    ('variables/flavors', 2, [('foo += $(bar)', 'foo = $(bar)')], 'changed', 'changed', 'append replaced by assignment'),
]

PUPPET_EDITS = [
    # (spec file, extracted manifest value, edits, claim, kind, description). The spec manifests that compile on
    # their own are few: most use interpolation, undeclared classes or references. `notify` needs no provider.
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }', [('": }', '": message => \'hello\' }')], 'changed',
     'changed', 'message parameter added'),
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }', [('"thisisa[bug]"', "'thisisa[bug]'")], 'preserved',
     'equivalent', 'double-quoted title respelt single-quoted (no interpolation)'),
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }', [('notify { "thisisa[bug]": }', 'notify {\n  "thisisa[bug]":\n}')],
     'preserved', 'style', 'resource body spread over several lines'),
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }', [('notify {', '# probe\nnotify {')], 'preserved', 'comment',
     'comment line added'),
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }', [('thisisa[bug]', 'thisisa[feature]')], 'changed', 'changed',
     'resource title changed'),
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }', [('": }', '": ; }')], 'preserved', 'equivalent',
     'trailing `;` after the last resource body (PUP-2898)'),
    ('parse_resource_spec.rb', 'notify { "thisisa[bug]": }',
     [('notify { "thisisa[bug]": }', 'notify { "thisisa[bug]": }\nnotify { \'other\': }\nNotify[\'other\'] -> Notify["thisisa[bug]"]')],
     'changed', 'changed', 'second resource and an ordering arrow added'),
]


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _slug(text):
    return re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')


def _apply(text, edits, where):
    for old, new in edits:
        if text.count(old) != 1:
            raise ValueError(f'{where}: edit anchor must occur exactly once: {old!r}')
        text = text.replace(old, new)
    return text


def txtar_members(data: bytes):
    """Split a txtar archive into (comment, [(name, bytes)]) exactly as golang.org/x/tools/txtar does."""
    members, name, start, comment_end = [], None, None, None
    pos = 0
    marker = re.compile(rb'^-- (.+?) --$')
    lines = data.splitlines(keepends=True)
    for line in lines:
        m = marker.match(line.rstrip(b'\r\n')) if line.endswith(b'\n') or line == lines[-1] else None
        if m:
            if name is not None:
                members.append((name, data[start:pos]))
            elif comment_end is None:
                comment_end = pos
            name, start = m.group(1).decode(), pos + len(line)
        pos += len(line)
    if name is not None:
        members.append((name, data[start:pos]))
    return members


def heredocs(script: str):
    """Quoted here-documents (`<<-\\EOF`) of t0008, as the shell writes them: leading tabs removed."""
    out = []
    lines = script.splitlines(keepends=True)
    i = 0
    pat = re.compile(r"^\t*(?:cat <<-\\EOF >(\S+)|cat >(\S+) <<-\\EOF|sed -e \"s/Z\$//\" >(\S+) <<-\\EOF)")
    while i < len(lines):
        m = pat.match(lines[i])
        if not m:
            i += 1
            continue
        target = m.group(1) or m.group(2) or m.group(3)
        body, j = [], i + 1
        while not re.match(r'^\t*EOF$', lines[j].rstrip('\n')):
            body.append(lines[j].lstrip('\t'))
            j += 1
        payload = ''.join(body)
        if m.group(3):
            payload = ''.join(re.sub(r'Z$', '', line.rstrip('\n')) + '\n' for line in body)
        out.append({'line': i + 1, 'target': target, 'payload': payload, 'sed': bool(m.group(3))})
        i = j + 1
    return out


def plan(root: Path, write):
    """Every #267 case: bytes, claim and witness plan. ``write(path, text)`` stores derived bytes."""
    root = Path(root)
    cases = []

    def ref(path):
        return {'path': path, 'sha256': _sha((root / path).read_bytes())}

    def add(**kw):
        kw.setdefault('derivation', [{'step': 'verbatim_upstream', 'description': 'exact upstream file bytes at the '
                                      'pinned revision on both sides'}])
        cases.append(kw)

    def derived(language, sub, name, text):
        return write(f'admission/cases/{language}/{sub}/{name}', text)

    # ------------------------------------------------------------ dockerfile (BuildKit)
    bk = f'{UP}/buildkit-dockerfile-parser/frontend/dockerfile/parser'
    for d in sorted(p.name for p in (root / bk / 'testfiles').iterdir()):
        doc = f'{bk}/testfiles/{d}/Dockerfile'
        data = (root / doc).read_bytes()
        add(id=f'bk-{_slug(d)}-parse', suite='buildkit-dockerfile-parser', language='dockerfile',
            filename='Dockerfile', lane='parser_fidelity', family='parse_acceptance', old=ref(doc), new=ref(doc),
            upstream_id=f'frontend/dockerfile/parser/testfiles/{d}', record=ref(f'{bk}/testfiles/{d}/result'),
            fields=['Dockerfile', 'result (expected AST dump)'], claim='preserved', accepts=True,
            inventory=b'#' not in data,
            assumptions='TestParseCases: parser.Parse accepts the file and AST.Dump() + "\\n" equals the upstream '
                        '`result` file; identity control in the parser-fidelity lane.',
            plan={'oracle': 'buildkit-parse', 'expect': 'accept', 'result': f'{bk}/testfiles/{d}/result'})
    line_doc = f'{bk}/testfile-line/Dockerfile'
    add(id='bk-testfile-line-parse', suite='buildkit-dockerfile-parser', language='dockerfile', filename='Dockerfile',
        lane='parser_fidelity', family='parse_acceptance', old=ref(line_doc), new=ref(line_doc),
        upstream_id='frontend/dockerfile/parser/testfile-line/Dockerfile', record=ref(line_doc), fields=['Dockerfile'],
        claim='preserved', accepts=True, inventory=b'#' not in (root / line_doc).read_bytes(),
        assumptions='TestParseLineInfo parses this file without error.',
        plan={'oracle': 'buildkit-parse', 'expect': 'accept', 'result': None})
    for d in ('env_no_value', 'shykes-nested-json'):
        doc = f'{bk}/testfiles-negative/{d}/Dockerfile'
        add(id=f'bk-negative-{_slug(d)}', suite='buildkit-dockerfile-parser', language='dockerfile',
            filename='Dockerfile', lane='parser_fidelity', family='parse_rejection', old=ref(doc), new=ref(doc),
            upstream_id=f'frontend/dockerfile/parser/testfiles-negative/{d}', record=ref(doc), fields=['Dockerfile'],
            claim='preserved', accepts=False, inventory=False,
            assumptions='TestParseErrorCases: parser.Parse must return an error.',
            plan={'oracle': 'buildkit-parse', 'expect': 'reject', 'result': None})
    for seed, name, edits, claim, kind, what in DOCKER_EDITS:
        src = f'{bk}/testfiles/{seed}/Dockerfile'
        text = (root / src).read_text(encoding='utf-8')
        cid = f'bk-{_slug(seed)}-{name}'
        new = derived('dockerfile', 'buildkit', f'{_slug(seed)}-{name}.Dockerfile', _apply(text, edits, cid))
        add(id=cid, suite='buildkit-dockerfile-parser', language='dockerfile', filename='Dockerfile',
            lane='change_understanding', family=name.replace('-', '_'), old=ref(src), new=new,
            upstream_id=f'frontend/dockerfile/parser/testfiles/{seed}', record=ref(f'{bk}/testfiles/{seed}/result'),
            fields=['Dockerfile'], claim=claim, kind=kind,
            derivation=[{'step': 'derived_transformation', 'description': f'{what} in upstream testfiles/{seed}/Dockerfile '
                         f'(old = upstream bytes)', 'tool': TOOL}],
            assumptions='Observation domain: the BuildKit v0.17.3 parsed instruction list (instructions.Parse typed '
                        'commands per stage, without source locations, original text or the legacy-form marker). '
                        'No image is built and no base image is resolved.',
            plan={'oracle': 'buildkit-pair'})

    # ------------------------------------------------------------ hcl (spec suite + Terraform testdata)
    for t in sorted((root / UP / HS).rglob('*.hcl')):
        doc = str(t.relative_to(root))
        rel = str(t.relative_to(root / UP / HS))
        tfile = t.with_suffix('.t')
        expects_error = tfile.exists() and 'diagnostics {' in tfile.read_text()
        add(id=f'hclspec-{_slug(rel[:-4])}-parse', suite='hcl-specsuite', language='hcl', filename='fixture.hcl',
            lane='parser_fidelity', family='parse_acceptance', old=ref(doc), new=ref(doc),
            upstream_id=f'specsuite/tests/{rel}', record=ref(str(tfile.relative_to(root))) if tfile.exists() else ref(doc),
            fields=['.hcl document', '.t expected diagnostics'], claim='preserved', accepts=None,
            inventory=not re.search(rb'#|//|/\*', (root / doc).read_bytes()),
            assumptions='Native-syntax acceptance by hclsyntax.ParseConfig v2.23.0. The upstream .t file '
                        + ('expects error diagnostics; only syntax diagnostics make the document must-reject, '
                           'schema diagnostics (hcldec) do not.' if expects_error else 'expects no diagnostics.'),
            plan={'oracle': 'hcl-parse', 'expect': 'reference', 'upstream_expects_error': expects_error})
    tfd = f'{UP}/terraform-configs-testdata/internal/configs/testdata'
    for kind_dir in ('valid-files', 'invalid-files'):
        for t in sorted((root / tfd / kind_dir).glob('*.tf')):
            doc = str(t.relative_to(root))
            add(id=f'tf-{kind_dir.split("-")[0]}-{_slug(t.stem)}-parse', suite='terraform-configs-testdata',
                language='hcl', filename='main.tf', lane='parser_fidelity', family='parse_acceptance',
                old=ref(doc), new=ref(doc), upstream_id=f'internal/configs/testdata/{kind_dir}/{t.name}',
                record=ref(doc), fields=['.tf file', 'directory label ' + kind_dir], claim='preserved', accepts=None,
                inventory=not re.search(rb'#|//|/\*', t.read_bytes()),
                assumptions=('Terraform 1.5.7 loads this file without error (valid-files); the native-syntax claim '
                             'is established by hclsyntax.ParseConfig.') if kind_dir == 'valid-files' else
                ('Terraform 1.5.7 rejects this configuration (invalid-files). The parser-fidelity claim is HCL '
                 'native-syntax acceptance by hclsyntax.ParseConfig: files whose error is a Terraform schema/semantic '
                 'error are syntactically valid HCL and must be accepted; files with syntax errors must be rejected.'),
                plan={'oracle': 'hcl-parse', 'expect': 'reference', 'upstream_label': kind_dir})
    for src, name, edits, claim, kind, what in HCL_EDITS:
        text = (root / UP / src).read_text(encoding='utf-8')
        stem = _slug(Path(src).stem)
        cid = f'hcl-{stem}-{name}'
        ext = Path(src).suffix
        new = derived('hcl', 'derived', f'{stem}-{name}{ext}', _apply(text, edits, cid))
        add(id=cid, suite='terraform-configs-testdata' if src.startswith('terraform') else 'hcl-specsuite',
            language='hcl', filename='main.tf' if ext == '.tf' else 'fixture.hcl', lane='change_understanding',
            family=name.replace('-', '_'), old=ref(f'{UP}/{src}'), new=new, upstream_id=src.split('/', 1)[1],
            record=ref(f'{UP}/{src}'), fields=['source file'], claim=claim, kind=kind,
            derivation=[{'step': 'derived_transformation', 'description': f'{what} (old = upstream bytes)', 'tool': TOOL}],
            assumptions='Observation domain: the HCL information model from hclsyntax v2.23.0 - attributes as an '
                        'unordered map of typed constant values (cty JSON with type) or, for non-constant expressions, '
                        'their token sequence without comments/newlines; blocks as an ordered sequence of type, labels '
                        'and body. No Terraform schema, provider or state is applied.',
            plan={'oracle': 'hcl-pair'})
    for src in HCL_FMT_SEEDS:
        stem = _slug(Path(src).stem)
        cid = f'hcl-{stem}-hclfmt'
        add(id=cid, suite='terraform-configs-testdata' if src.startswith('terraform') else 'hcl-specsuite',
            language='hcl', filename='main.tf' if src.endswith('.tf') else 'fixture.hcl', lane='change_understanding',
            family='canonical_format', old=ref(f'{UP}/{src}'), new=None, new_from_tool=('hclfmt', f'{UP}/{src}',
                                                                                         f'admission/cases/hcl/hclfmt/{stem}{Path(src).suffix}'),
            upstream_id=src.split('/', 1)[1], record=ref(f'{UP}/{src}'), fields=['source file'], claim='preserved',
            kind='style',
            derivation=[{'step': 'derived_transformation', 'description': 'canonical formatting by hclwrite.Format '
                         'v2.23.0 (the formatter behind terraform fmt); old = upstream bytes',
                         'tool': 'scripts/witness_tools/buildinfra (hclfmt)'}],
            assumptions='hclwrite.Format only changes whitespace and alignment; information-model equality is '
                        'checked by the witness.', plan={'oracle': 'hcl-pair'})

    # ------------------------------------------------------------ gomod (x/mod + cmd/go testdata)
    for t in sorted((root / UP / XM).glob('*')):
        if not t.is_file() or t.suffix not in ('.in', '.golden'):
            continue
        doc = str(t.relative_to(root))
        add(id=f'xmod-{_slug(t.name)}-parse', suite='x-mod-modfile', language='gomod', filename='go.mod',
            lane='parser_fidelity', family='parse_acceptance', old=ref(doc), new=ref(doc),
            upstream_id=f'modfile/testdata/{t.name}', record=ref(doc), fields=[t.suffix[1:] + ' file'],
            claim='preserved', accepts=True, inventory=b'//' not in t.read_bytes(),
            assumptions='x/mod read_test parses every .in and .golden file; must be accepted by modfile.Parse and by '
                        'cmd/go (go mod edit -json).', plan={'oracle': 'modfile-parse', 'expect': 'accept'})
    for t in sorted((root / UP / XM).glob('*.in')):
        golden = t.with_suffix('.golden')
        if not golden.exists():
            continue
        add(id=f'xmod-{_slug(t.stem)}-golden', suite='x-mod-modfile', language='gomod', filename='go.mod',
            lane='change_understanding', family='canonical_format', old=ref(str(t.relative_to(root))),
            new=ref(str(golden.relative_to(root))), upstream_id=f'modfile/testdata/{t.stem}',
            record=ref(str(golden.relative_to(root))), fields=['.in', '.golden'], claim='preserved', kind='style',
            derivation=[{'step': 'verbatim_upstream', 'description': f'upstream pair {t.name} -> {golden.name} '
                         '(TestPrintGolden: Format of the parsed .in equals .golden), unmodified'}],
            assumptions='Observation domain: cmd/go go1.24.7 `go mod edit -json` (module, go, toolchain, require with '
                        'indirect flags, exclude, replace, retract, godebug, tool). Comments are not observed.',
            plan={'oracle': 'modfile-pair'})
    seen = {}
    td = f'{UP}/go-cmd-mod-testdata/src/cmd/go/testdata/mod'
    for t in sorted((root / td).glob('*.txt')):
        for name, data in txtar_members(t.read_bytes()):
            if name not in ('.mod', 'go.mod') or data in seen:
                continue
            try:
                text = data.decode('utf-8')
            except UnicodeDecodeError:
                continue
            seen[data] = t.name
            cid = f'cmdgo-{_slug(t.stem)}-{_slug(name) or "mod"}-parse'
            doc = derived('gomod', 'cmdgo', f'{t.stem}{name.replace("/", "_")}', text)
            add(id=cid, suite='go-cmd-mod-testdata', language='gomod', filename='go.mod', lane='parser_fidelity',
                family='parse_acceptance', old=doc, new=doc, upstream_id=f'src/cmd/go/testdata/mod/{t.name}#{name}',
                record=ref(str(t.relative_to(root))), fields=[f'txtar member {name}'], claim='preserved', accepts=True,
                inventory=b'//' not in data,
                derivation=[{'step': 'verbatim_upstream', 'description': f'txtar member `{name}` of {t.name}, '
                             'extracted byte-exact (the bytes cmd/go serves for this module version)', 'tool': TOOL}],
                assumptions='The module proxy test server serves this member as the module version go.mod; cmd/go '
                            'must parse it. Members are deduplicated by bytes.',
                plan={'oracle': 'modfile-parse', 'expect': 'accept'})
    script = f'{UP}/go-cmd-mod-testdata/src/cmd/go/testdata/script/mod_replace.txt'
    gomod_seed = dict(txtar_members((root / script).read_bytes()))['go.mod'].decode()
    if gomod_seed != GOMOD_REPLACE:
        raise ValueError('mod_replace.txt go.mod precondition failed')
    seed_ref = derived('gomod', 'derived', 'mod_replace-go.mod', gomod_seed)
    for seed, name, edits, claim, kind, what in GOMOD_EDITS:
        if seed == 'mod_replace':
            old, text, record, uid = seed_ref, gomod_seed, ref(script), 'src/cmd/go/testdata/script/mod_replace.txt#go.mod'
        else:
            old, text, record, uid = ref(f'{UP}/{seed}'), (root / UP / seed).read_text(), ref(f'{UP}/{seed}'), seed.split('/', 1)[1]
        cid = f'gomod-{_slug(Path(seed).stem)}-{name}'
        new = derived('gomod', 'derived', f'{_slug(Path(seed).stem)}-{name}.mod', _apply(text, edits, cid))
        add(id=cid, suite='go-cmd-mod-testdata' if seed == 'mod_replace' else 'x-mod-modfile', language='gomod',
            filename='go.mod', lane='change_understanding', family=name.replace('-', '_'), old=old, new=new,
            upstream_id=uid, record=record, fields=['go.mod'], claim=claim, kind=kind,
            derivation=([{'step': 'derived_transformation', 'description': 'txtar member go.mod of mod_replace.txt '
                          'extracted byte-exact (old)', 'tool': TOOL}] if seed == 'mod_replace' else [])
            + [{'step': 'derived_transformation', 'description': what, 'tool': TOOL}],
            assumptions='Observation domain: cmd/go go1.24.7 `go mod edit -json`; comments other than `// indirect` '
                        'are not observed.', plan={'oracle': 'modfile-pair'})

    # ------------------------------------------------------------ gitignore (git t0008)
    script_text = (root / UP / T0008).read_text(encoding='utf-8')
    docs = heredocs(script_text)
    wanted = {'.gitignore', 'a/.gitignore', 'a/b/.gitignore', '"$global_excludes"', '.git/info/exclude', 'ignore'}
    setup = {}
    for h in docs:
        if h['target'] not in wanted:
            continue
        if h['target'] == '.gitignore' and '.gitignore' in setup:
            target = 'data/.gitignore'  # the 'directories and ** matches' test rewrites .gitignore
        else:
            target = h['target']
        if target in setup:
            continue
        setup[target] = h
    names = {'.gitignore': 'setup-gitignore', 'a/.gitignore': 'setup-a-gitignore', 'a/b/.gitignore': 'setup-a-b-gitignore',
             '"$global_excludes"': 'setup-global-excludes', '.git/info/exclude': 'setup-info-exclude',
             'data/.gitignore': 'dirs-gitignore', 'ignore': 'whitespace-ignore'}
    git_refs = {}
    for target, h in sorted(setup.items(), key=lambda kv: kv[1]['line']):
        name = names[target]
        doc = derived('gitignore', 't0008', name, h['payload'])
        git_refs[target if target != '"$global_excludes"' else 'global-excludes'] = (doc, h['payload'])
        test = 'setup' if name.startswith('setup') else ('directories and ** matches' if name.startswith('dirs')
                                                         else 'correct handling of backslashes')
        add(id=f't0008-{name}-parse', suite='git-t0008-ignores', language='gitignore', filename='.gitignore',
            lane='parser_fidelity', family='parse_acceptance', old=doc, new=doc,
            upstream_id=f't/t0008-ignores.sh:{h["line"]} ({test}: {h["target"]})', record=ref(f'{UP}/{T0008}'),
            fields=['here-document payload'], claim='preserved', accepts=True, inventory=False,
            derivation=[{'step': 'derived_transformation', 'description': f'here-document at line {h["line"]} written '
                         'as the shell writes it: `<<-` removes leading tabs' + ('; then sed s/Z$// as the test pipes it'
                                                                                 if h['sed'] else ''), 'tool': TOOL}],
            assumptions='git accepts every line of an ignore file; the witness reproduces the upstream t0008 '
                        'expectation that uses this file with git 2.43.0.',
            plan={'oracle': 'git-t0008', 'test': test})
    for target, name, edits, claim, kind, what in GIT_EDITS:
        key = target
        doc, text = git_refs[key]
        cid = f't0008-{names[target if target in names else key]}-{name}'
        new = derived('gitignore', 'derived', f'{names[target]}-{name}', _apply(text, edits, cid))
        layout = 'data' if target == 'data/.gitignore' else 'setup'
        add(id=cid, suite='git-t0008-ignores', language='gitignore',
            filename='exclude' if target == '.git/info/exclude' else '.gitignore', lane='change_understanding',
            family=name.replace('-', '_'), old=doc, new=new, upstream_id=f't/t0008-ignores.sh ({target})',
            record=ref(f'{UP}/{T0008}'), fields=['here-document payload'], claim=claim, kind=kind,
            derivation=[{'step': 'derived_transformation', 'description': 't0008 here-document payload (old), as '
                         'extracted for the parser-fidelity case', 'tool': TOOL},
                        {'step': 'derived_transformation', 'description': what, 'tool': TOOL}],
            assumptions='Observation domain: the set of ignored paths reported by git 2.43.0 `check-ignore --no-index '
                        '-z --stdin` over a fixed universe of materialised files, in the t0008 ' + layout + ' layout '
                        '(all other ignore files unchanged). Verbose line numbers are not observed.',
            plan={'oracle': 'git-pair', 'layout': layout, 'target': target,
                  'universe': DATA_UNIVERSE if layout == 'data' else SETUP_UNIVERSE})

    # ------------------------------------------------------------ cmake (RunCMake Syntax)
    rc = f'{UP}/cmake-runcmake-syntax/Tests/RunCMake/Syntax'
    index = (root / rc / 'RunCMakeTest.cmake').read_text()
    for name in sorted(set(re.findall(r'^run_cmake\(([A-Za-z0-9_.+-]+)\)', index, re.M))):
        doc = f'{rc}/{name}.cmake'
        data = (root / doc).read_bytes()
        try:
            data.decode('utf-8')
        except UnicodeDecodeError:
            continue  # recorded in the discovery eligibility rule
        stderr = root / rc / f'{name}-stderr.txt'
        expect = 'reject' if stderr.exists() and re.search(r'Parse error|Syntax Error in cmake code',
                                                            stderr.read_text(errors='replace')) else 'accept'
        add(id=f'runcmake-syntax-{_slug(name)}', suite='cmake-runcmake-syntax', language='cmake',
            filename='fixture.cmake', lane='parser_fidelity',
            family='parse_rejection' if expect == 'reject' else 'parse_acceptance', old=ref(doc), new=ref(doc),
            upstream_id=f'Tests/RunCMake/Syntax: run_cmake({name})',
            record=ref(str(stderr.relative_to(root))) if stderr.exists() else ref(doc),
            fields=[f'{name}.cmake', 'expected result/stdout/stderr'], claim='preserved', accepts=expect == 'accept',
            inventory=False,
            assumptions=('The upstream expected stderr reports a CMake language parse error in this file: the '
                         'document must be rejected.' if expect == 'reject' else
                         'No parse error is expected upstream (the file may still fail at configure time, e.g. '
                         'message(FATAL_ERROR)): the document must be accepted.'),
            plan={'oracle': 'runcmake', 'test': name, 'expect': expect})
    for name, edit, edits, claim, kind, what in CMAKE_EDITS:
        src = f'{rc}/{name}.cmake'
        cid = f'runcmake-{_slug(name)}-{edit}'
        new = derived('cmake', 'derived', f'{name}-{edit}.cmake', _apply((root / src).read_text(), edits, cid))
        add(id=cid, suite='cmake-runcmake-syntax', language='cmake', filename='fixture.cmake',
            lane='change_understanding', family=edit.replace('-', '_'), old=ref(src), new=new,
            upstream_id=f'Tests/RunCMake/Syntax: run_cmake({name})', record=ref(src), fields=[f'{name}.cmake'],
            claim=claim, kind=kind,
            derivation=[{'step': 'derived_transformation', 'description': what + ' (old = upstream bytes)', 'tool': TOOL}],
            assumptions='Observation domain: exit status, stdout and stderr of cmake 3.28.3 configuring the upstream '
                        'Syntax/CMakeLists.txt with RunCMake_TEST=<case> (Unix Makefiles, project NONE), with the '
                        'build path normalised. Source text and positions are not introspected by these scripts.',
            plan={'oracle': 'cmake-pair'})

    # ------------------------------------------------------------ make (GNU Make harness) and puppet (specs)
    for language, index_path in (('make', 'admission/cases/make/harness/index.json'),
                                 ('puppet', 'admission/cases/puppet/specs/index.json')):
        if not (root / index_path).exists():
            continue
        entries = json.loads((root / index_path).read_bytes())
        for e in entries['documents']:
            doc = ref(e['path'])
            if language == 'make':
                upstream_id = f"tests/scripts/{e['script']} test {e['index']} (line {e['line']})"
                record = ref(f"{UP}/gnu-make-tests/tests/scripts/{e['script']}")
                assumption = ('Makefile written by the upstream run_make_tests.pl harness for this run_make_test call; '
                              'the harness run on GNU Make 4.3 reproduced the upstream expected output and exit '
                              'status. ' + ('The expected output is a makefile read error: the document must be '
                                            'rejected.' if e['expect'] == 'reject' else 'No makefile read error is '
                                            'expected: the document must be accepted.'))
                fields = ['run_make_test makefile', 'expected output', 'expected exit status']
            else:
                upstream_id = f"spec/unit/pops/parser/{e['spec']}:{e['line']}"
                record = ref(f"{UP}/puppet-parser-specs/spec/unit/pops/parser/{e['spec']}")
                assumption = ('The spec expects `parse(...)` to ' + ('raise an error' if e['expect'] == 'reject' else
                                                                    'succeed') + ' (Puppet::Pops::Parser::Parser).')
                fields = ['string literal passed to parse()', 'spec expectation']
            add(id=e['id'], suite='gnu-make-tests' if language == 'make' else 'puppet-parser-specs', language=language,
                filename='Makefile' if language == 'make' else 'fixture.pp', lane='parser_fidelity',
                family='parse_rejection' if e['expect'] == 'reject' else 'parse_acceptance', old=doc, new=doc,
                upstream_id=upstream_id, record=record, fields=fields, claim='preserved',
                accepts=e['expect'] == 'accept', inventory=False,
                derivation=[{'step': 'verbatim_upstream' if e['verbatim'] else 'derived_transformation',
                             'description': e['derivation'], 'tool': 'scripts/build_infra_witness.py extract'}],
                assumptions=assumption,
                plan={'oracle': 'make-harness' if language == 'make' else 'puppet-parse', 'expect': e['expect'],
                      'entry': e})
        by_key = {(e.get('script'), e.get('index')) if language == 'make' else (e['spec'], e['value']): e
                  for e in entries['documents']}
        for spec in (MAKE_EDITS if language == 'make' else PUPPET_EDITS):
            src_key, sub, edits, claim, kind, what = spec[0], spec[1], spec[2], spec[3], spec[4], spec[5]
            e = by_key.get((src_key, sub))
            if e is None:
                raise ValueError(f'{language}: seed not extracted: {src_key} {sub}')
            text = (root / e['path']).read_text(encoding='utf-8')
            slug = _slug(edits[0][1])[:24]
            cid = f"{e['id']}-{_slug(kind)}-{_sha((what + repr(edits)).encode())[:6]}"
            new = derived(language, 'derived', cid + ('.mk' if language == 'make' else '.pp'), _apply(text, edits, cid))
            add(id=cid, suite='gnu-make-tests' if language == 'make' else 'puppet-parser-specs', language=language,
                filename='Makefile' if language == 'make' else 'fixture.pp', lane='change_understanding',
                family=kind, old=ref(e['path']), new=new,
                upstream_id=(f"tests/scripts/{e['script']} test {e['index']}" if language == 'make'
                             else f"spec/unit/pops/parser/{e['spec']}:{e['line']}"),
                record=ref(e['path']), fields=['extracted document'], claim=claim, kind=kind,
                derivation=[{'step': 'derived_transformation', 'description': f'{what} (old = the extracted upstream '
                             'document)', 'tool': TOOL}],
                assumptions=('Observation domain: stdout, stderr and exit status of GNU Make 4.3 run with the '
                             'upstream test options in an empty directory (the test\'s own file setup is not '
                             'replayed), with the makefile path normalised.' if language == 'make' else
                             'Observation domain: the catalog Puppet 8.10.0 compiles for a fixed node and facts - '
                             'resources with their parameters, plus ordering and refresh edge sets normalised from '
                             'metaparameters and relationship arrows. No provider is run.'),
                plan={'oracle': 'make-pair' if language == 'make' else 'puppet-pair',
                      'options': e.get('options') if language == 'make' else None})
    return cases


def _expected(ctx, case, witness_record):
    classification, forbid = ctx.classification, ctx.forbid
    if case['lane'] == 'parser_fidelity':
        if not case['accepts']:
            return {'classification': classification(None, None, None), 'required': [], 'alternatives': [],
                    'forbidden': [], 'parse': {'accepts': False}}
        inventory = None
        starts = (witness_record or {}).get('unit_starts')
        if case.get('inventory') and starts:
            inventory = {'count': len(starts), 'starts': starts,
                         'evidence': f"Reference inventory: top-level units reported by {witness_record['tool']} "
                                     '(start of each instruction / attribute or block / statement) in the exact bytes. '
                                     'Only documents without comments carry an inventory, because the comparison domain '
                                     'cannot tell a comment node from a statement without node type names.'}
        return {'classification': classification(False, None, True), 'required': [], 'alternatives': [],
                'forbidden': [forbid('change_type', 'byte-identical input has no edit', t) for t in ALL_TYPES]
                + [forbid('fallback', 'reference-accepted source must be parsed, not fall back')],
                'parse': {'accepts': True, 'statement_inventory': inventory}}
    old = (ctx.ROOT / case['old']['path']).read_bytes(); new = (ctx.ROOT / case['new']['path']).read_bytes()
    kind = case['kind']
    if kind == 'changed':
        old_r, new_r = ctx.minimal_region(old, new)
        return {'classification': classification(True, False, False), 'alternatives': [],
                'required': [{'kind': 'edit_region', 'change_types_any_of': EDIT_TYPES, 'old_range': old_r,
                              'new_range': new_r, 'evidence': f'witnessed change: a reported edit must touch bytes '
                              f'{old_r["start_byte"]}..{old_r["end_byte"]} (old) / {new_r["start_byte"]}..'
                              f'{new_r["end_byte"]} (new)'}],
                'forbidden': [forbid('style_only', 'a witnessed change in the observation domain is not style-only'),
                              forbid('no_changes', 'the edit must be reported'),
                              forbid('fallback', 'valid source must be reviewed, not fall back')]}
    if kind == 'style':
        return {'classification': classification(False, True, None), 'required': [], 'alternatives': [],
                'forbidden': [forbid('change_type', 'presentation-only edit is not a code edit', t) for t in CODE_TYPES]
                + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
    if kind == 'comment':
        return {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
                'forbidden': [forbid('change_type', 'a comment is not a code edit', t) for t in CODE_TYPES]
                + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
    if kind == 'equivalent':
        return {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
                'forbidden': [forbid('change_type', 'an equivalent spelling changes no value', t)
                              for t in ('ADDITION', 'DELETION', 'MODIFICATION')]
                + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
    if kind == 'reorder':
        return {'classification': classification(False, None, None), 'required': [], 'alternatives': [],
                'forbidden': [forbid('change_type', 'reordering unordered members adds, deletes or modifies nothing', t)
                              for t in ('ADDITION', 'DELETION', 'MODIFICATION')]
                + [forbid('fallback', 'valid source must be reviewed, not fall back')]}
    raise ValueError(f'unknown expectation kind {kind}')


def contribute(ctx):
    root = Path(ctx.ROOT)

    def ref(path):
        return ctx.ref(path)

    planned = plan(root, ctx.write)
    cases = []
    for p in planned:
        if p['new'] is None:  # bytes produced by the reference formatter at witness time
            tool, _, path = p['new_from_tool']
            if not (root / path).exists():
                continue
            p['new'] = ref(path)
        wpath = f"{WITNESSES}/{p['id']}.json"
        record = json.loads((root / wpath).read_bytes()) if (root / wpath).exists() else None
        witness = ref(wpath) if record else None
        verdict = record['verdict'] if record else None
        if record and (record['old_sha256'] != p['old']['sha256'] or record['new_sha256'] != p['new']['sha256']):
            record, witness, verdict = None, None, None  # stale witness: not admitted
        if p['lane'] == 'parser_fidelity' and p.get('accepts') is None and record:
            p['accepts'] = bool(record.get('reference_accepts'))
        admitted = verdict == 'supports_claim' and p.get('accepts', True) is not None
        verbatim = p['derivation'][0]['step'] == 'verbatim_upstream'
        if admitted:
            cls, status = ('upstream_gold' if verbatim else 'reviewed_external_derived'), 'admitted'
            reason = (record.get('review') or 'Reference outcome reproduced.') + f' Witness verdict: {verdict}.'
        else:
            cls, status = 'external_seed_only', 'candidate'
            reason = (f"Witness verdict {verdict}: {record.get('review', '')}; not admitted." if record
                      else 'No executed witness for these bytes; not admitted.')
        meta = SUITES[p['suite']]
        case = {'id': p['id'], 'suite_id': p['suite'], 'language_id': p['language'], 'dialect': meta['dialect'],
                'filename': p['filename'], 'class': cls, 'lane': p['lane'], 'change_family': p['family'],
                'upstream': {'case_id': p['upstream_id'], 'fields': p['fields'], 'record': p['record']},
                'old': p['old'], 'new': p['new'], 'licence': meta['licence'], 'attribution': meta['attribution'],
                'derivation': p['derivation'],
                'behavior': {'claim': p['claim'], 'assumptions': p['assumptions']},
                'oracle': {'basis': 'executable_witness' if witness else 'candidate_only', 'witness': witness,
                           'byte_identity': p['old'] == p['new'], 'notes': WITNESS_NOTE},
                'admission': {'status': status, 'reason': reason},
                'expected': _expected(ctx, p, record) if admitted else None,
                'witness_plan': p['plan']}
        cases.append(case)
    suites = []
    for suite_id, meta in SUITES.items():
        directory = root / UP / suite_id
        files = sorted(q for q in directory.rglob('*') if q.is_file())
        acq = json.loads((directory / 'ACQUISITION.json').read_bytes())
        suites.append({'suite_id': suite_id, 'name': meta['name'], 'version': meta['version'], 'url': meta['url'],
                       'revision': meta['revision'], 'licence': meta['licence'],
                       'licence_evidence': meta['licence_evidence'], 'attribution': meta['attribution'],
                       'evidence_class': meta['evidence_class'],
                       'acquisition': {'method': 'scripts/acquire_build_infra_benchmarks.py: ' + (
                           f"raw.githubusercontent.com/{acq['repo']}/{acq['revision']}/<path>" if acq['method'] == 'raw'
                           else f"go mod download {acq['module']}@{acq['version']} (zip sha256 {acq['zip_sha256']}, "
                                f"{acq['module_sum']})") + ' (2026-11-17)',
                           'files': [ref(str(q.relative_to(root))) for q in files],
                           'verification': acq['verification'] + [f"enumeration: {acq['enumeration']['method']}"]}})
    discovery = []
    counts = {}
    for c in cases:
        # Discovery counts upstream documents/pairs only: every parser-fidelity case is one upstream document
        # (verbatim or extracted); a change pair counts only when the pair itself is upstream.
        key = (c['suite_id'], c['language_id'], c['lane'])
        counts.setdefault(key, [0, 0])
        if c['lane'] == 'parser_fidelity' or c['derivation'][0]['step'] == 'verbatim_upstream':
            counts[key][0] += 1
            counts[key][1] += c['admission']['status'] == 'admitted'
    rules = {
        ('buildkit-dockerfile-parser', 'parser_fidelity'): (
            'testfiles/*/Dockerfile (must accept, with result), testfile-line/Dockerfile and testfiles-negative/* '
            '(must reject). Excluded: testfiles-negative/empty_dockerfile and only_comments, whose upstream error is '
            '"file with no instructions" (an empty document, not a syntax error). Inventory only without `#`.', 2),
        ('hcl-specsuite', 'parser_fidelity'): ('every specsuite/tests/**/*.hcl document', 0),
        ('terraform-configs-testdata', 'parser_fidelity'): (
            'valid-files/*.tf and invalid-files/*.tf; .tf.json files are JSON-syntax HCL and route to another '
            'language, so they are excluded', 0),
        ('x-mod-modfile', 'parser_fidelity'): ('modfile/testdata/*.in and *.golden (the work/ go.work files excluded)', 0),
        ('go-cmd-mod-testdata', 'parser_fidelity'): (
            'UTF-8 `.mod`/`go.mod` txtar members of src/cmd/go/testdata/mod/*.txt, deduplicated by bytes', 0),
        ('git-t0008-ignores', 'parser_fidelity'): (
            'ignore files the script writes with quoted here-documents in the setup, `directories and ** matches` '
            'and `correct handling of backslashes` tests; files written by echo are not imported', 0),
        ('cmake-runcmake-syntax', 'parser_fidelity'): (
            'run_cmake(<name>) tests of Syntax/RunCMakeTest.cmake whose <name>.cmake is UTF-8 (the BOM-UTF-16/32 '
            'files are excluded); run_cmake_command/run_override tests are not imported', 0),
        ('gnu-make-tests', 'parser_fidelity'): (
            'run_make_test calls with a makefile string in the selected scripts (variables/flavors, define, '
            'automatic, negative, special; features/order_only, conditionals, targetvars, comments, escape; '
            'functions/foreach) whose harness run reproduced the upstream expectation as root on Linux', 0),
        ('puppet-parser-specs', 'parser_fidelity'): (
            'parse(<string literal>) calls without interpolation in the selected parser specs, inside an expect '
            'with a success or raise_error matcher (heredoc and interpolated literals are not extracted)', 0),
    }
    for (suite, language, lane), (n, admitted) in sorted(counts.items()):
        rule, extra = rules.get((suite, lane), (
            'upstream before/after pairs (x/mod .in -> .golden)' if n else
            'the suite has no before/after pairs; every change case is a reviewed derivation of a suite file and '
            'counts itself', 0))
        discovery.append({'suite_id': suite, 'language_id': language, 'lane': lane, 'discovered': n + extra,
                          'eligible': n, 'eligibility_rule': rule,
                          'imported_rows': 'all eligible',
                          'holdout': 'none; derived pairs are grouped by their upstream seed file'})
    return {'suites': suites, 'discovery': discovery, 'cases': cases, 'blocked_acquisitions': BLOCKED}
