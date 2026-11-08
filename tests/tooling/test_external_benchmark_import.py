import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'scripts/import_external_benchmarks.py'

class ImportTests(unittest.TestCase):
    def module(self):
        spec = importlib.util.spec_from_file_location('external_import', SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_safe_paths(self):
        m = self.module()
        for path in ('../escape', '/absolute', 'a/../../b', ''):
            with self.assertRaises(ValueError):
                m.safe_path(Path('/tmp'), path)

    def test_hash_validation_and_missing_provenance(self):
        m = self.module()
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / 'a').write_bytes(b'hello')
            source = {'id': 's', 'url': 'https://example.org', 'revision': 'abc', 'license': 'MIT', 'files': [{'path':'a', 'sha256':m.sha(b'hello')}]}
            manifest = {'schema_version':1, 'sources':[source], 'cases':[]}
            m.validate(root, manifest)
            (root / 'a').write_bytes(b'changed')
            with self.assertRaises(ValueError): m.validate(root, manifest)
            source.pop('license')
            with self.assertRaises(ValueError): m.validate(root, manifest)

    def test_checked_in_corpus_valid_and_deterministic(self):
        m = self.module()
        root = SCRIPT.parents[1] / 'tests/fixtures/external'
        import json
        manifest = json.loads((root/'manifest.json').read_text())
        m.validate(root, manifest)
        self.assertEqual(m.encode(manifest), (root/'manifest.json').read_bytes())
        self.assertEqual(len([c for c in manifest['cases'] if c['family']=='repair']),72)
        self.assertEqual(len([c for c in manifest['cases'] if c['family']=='identity']),72)

    def test_reviewed_controls_have_independent_python_syntax_evidence(self):
        import ast
        root = SCRIPT.parents[1] / 'tests/fixtures/external/cases/controls'
        before = ast.parse((root/'formatting-old.py').read_text())
        after = ast.parse((root/'formatting-new.py').read_text())
        self.assertEqual(ast.dump(before), ast.dump(after))
        renamed = ast.parse((root/'rename-new.py').read_text())
        self.assertEqual(renamed.body[-1].name, 'nearby_elements')
        renamed.body[-1].name = before.body[-1].name
        self.assertEqual(ast.dump(before), ast.dump(renamed))

    def test_full_import_is_reproducible_from_retained_parquet(self):
        try:
            import pyarrow
        except ImportError:
            self.skipTest('pyarrow needed for optional full importer replay')
        m = self.module()
        root = SCRIPT.parents[1] / 'tests/fixtures/external'
        with tempfile.TemporaryDirectory() as d:
            output = Path(d)
            (output/'upstream').mkdir()
            (output/'upstream/selection-issue-268.md').write_bytes((root/'upstream/selection-issue-268.md').read_bytes())
            m.build(root/'upstream', root/'inventory.json', output)
            self.assertEqual((output/'manifest.json').read_bytes(), (root/'manifest.json').read_bytes())
            self.assertEqual((output/'ledger.json').read_bytes(), (root/'ledger.json').read_bytes())

    def test_all_six_language_controls_have_exact_independent_deltas(self):
        import json
        import re
        root = SCRIPT.parents[1] / 'tests/fixtures/external'
        manifest=json.loads((root/'manifest.json').read_text())
        for case in manifest['cases']:
            if case['family'] not in ('rename','formatting'): continue
            old=(root/case['old_path']).read_text()
            new=(root/case['new_path']).read_text()
            if case['family']=='formatting':
                self.assertEqual(re.sub(r'\s+', '', old),re.sub(r'\s+', '',new))
            else:
                change=case['expected']['required_changes'][0]
                pattern=r'\b'+re.escape(change['old_label'])+r'\b'
                self.assertEqual(len(re.findall(pattern,old)),1)
                self.assertEqual(re.sub(pattern,change['new_label'],old),new)

    def test_cpp11_descendant_evidence_positions_are_original_literal_bytes(self):
        import json
        root=SCRIPT.parents[1]/'tests/fixtures/external'
        manifest=json.loads((root/'manifest.json').read_text())
        case=next(c for c in manifest['cases'] if c['id']=='hep-cpp-11-repair')
        facts=case['expected']['required_changes']
        self.assertEqual(len(facts),2)
        self.assertEqual([(f['old_label'],f['new_label']) for f in facts],[("'1'","'0'"),("'0'","'1'")])
        for f in facts:
            self.assertEqual(f['match'],'descendant_evidence')
            self.assertEqual(f['change_type'],'MODIFICATION')
            for side in ('old','new'):
                position=f[side+'_position']
                self.assertEqual(position['start_line'],position['end_line'])
                line=(root/case[side+'_path']).read_bytes().splitlines()[position['start_line']]
                self.assertEqual(line[position['start_col']:position['end_col']],f[side+'_label'].encode())
                self.assertTrue(line[:position['start_col']].rstrip().endswith(b'output+='))

    def test_preservation_forbids_actual_meaningful_change_types(self):
        import json
        root=SCRIPT.parents[1]/'tests/fixtures/external'
        manifest=json.loads((root/'manifest.json').read_text())
        for case in manifest['cases']:
            if case['family'] not in ('identity','regression_identity','regression_formatting'):continue
            forbidden=case['expected']['forbidden_change_types']
            self.assertIn('MOVE',forbidden)
            self.assertIn('REORDER',forbidden)
            self.assertNotIn('MOVED',forbidden)
            self.assertNotIn('STYLE_ONLY',forbidden)
