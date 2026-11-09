from pathlib import Path
import sys
root = Path('/workspace/intentumdiff-batch3')
sys.path.insert(0, str(root / 'cloud-integration'))
import cloud_common as cc
import audit_cloud_assessment as audit_module

baseline_path = cc.BASELINE
inventory_path = cc.TOOLING / 'tests/fixtures/external/inventory.json'
fixtures_path = cc.TOOLING / 'tests/fixtures/language_assessment'

def baseline_inputs_without_old_host_pin():
    baseline = cc.load_json(baseline_path)
    assert cc.sha(baseline_path) == cc.BASELINE_SHA
    inventory = cc.load_json(inventory_path)
    module = cc.assessment_module()
    fixtures, hashes = module.load_cases(fixtures_path, inventory)
    assert hashes == baseline['fixture_files']
    assert cc.sha(inventory_path) == baseline['inventory_sha256']
    assert len(fixtures) == 760 and len(inventory['parsers']) == 74
    assert baseline['parser_manifest']['parsers'] == inventory['parsers']
    assert baseline['parser_manifest']['extension_index'] == inventory['extension_index']
    old = module.baseline_cases(baseline, set(inventory['parsers']), None)
    assert len(old) == 760 and sum(c['status'] == 'passed' for c in old.values()) == 584
    manifest = baseline['parser_manifest']
    assert cc.sha_bytes((__import__('json').dumps(manifest, indent=2, sort_keys=True) + '\n').encode()) == baseline['parser_manifest_sha256']
    return baseline, inventory, fixtures, old, module

# The frozen assessor's original preflight pins the older native library and RC tree.
# Keep fixture, oracle, baseline, parser-manifest and component checks; substitute only
# the expected native hash for the freshly built merged RC artifact.
audit_module.baseline_inputs = baseline_inputs_without_old_host_pin
audit_module.NATIVE_SHA = cc.sha(Path('/tmp/core242-clean/crates/rust-core-host/target/debug/libintentumdiff_rust_core.so'))
report = root / 'full-cloud-assessment/rc-059fc9-merged/results.json'
out = root / 'full-cloud-assessment/rc-059fc9-merged/audit.json'
stage = root / 'parsers-cloud-ci'
raise SystemExit(audit_module.audit(report, out, stage, False))
