#!/usr/bin/env python3
"""Offline, deterministic HumanEvalPack import and provenance validation (test tooling only)."""
import argparse
import hashlib
import json
from pathlib import Path
import re
from collections import Counter

REVISION = '9a41762f73a8cb23bb5811b73d5aab164efcf378'
PARQUET_SHA256 = {'python': 'ed5f15d789156e21222bfcd556c425a39042355c84ae1e8b058abd6a3d7f8075', 'js': '52d6aada9ff62c22ba139aaa64431cca461fbca3cb0625521a988ffeef250855', 'java': '0be942d715894e3b350149f1cde6593e95eda495aa4bd1750f09428c0b3e3e9c', 'cpp': 'fd2eed15063bf883952fb3b0a23a372eeaa9b9e58932a496667a0c2595145779', 'rust': '02570ed6a11d8801d26a9aae0db87cdc0fc6f4a7eb68adb3a9e37fb4f752da57', 'go': '5cbb0e8152b5c9d2b595a1fa9d540c87cde7a306e9fc7f63febe24825bc20c74'}
LANGUAGES = {'python':('python','py'), 'js':('javascript','js'), 'java':('java','java'), 'go':('go','go'), 'cpp':('cpp','cpp'), 'rust':('rust','rs')}
def sha(data): return hashlib.sha256(data).hexdigest()
def encode(value): return (json.dumps(value, indent=2, ensure_ascii=False, sort_keys=True)+'\n').encode()
def safe_path(root, path):
    p = Path(path)
    if not path or p.is_absolute() or '..' in p.parts: raise ValueError(f'unsafe path: {path}')
    result = (root/p).resolve()
    if not result.is_relative_to(root.resolve()): raise ValueError(f'escaping path: {path}')
    return result

def validate(root, manifest):
    if manifest.get('schema_version') != 1: raise ValueError('schema_version')
    sources = {}
    for source in manifest['sources']:
        for key in ('id','url','revision','license','files'):
            if not source.get(key): raise ValueError(f'missing source {key}')
        if source['id'] in sources: raise ValueError('duplicate source')
        files = {}
        for f in source['files']:
            if sha(safe_path(root,f['path']).read_bytes()) != f['sha256']: raise ValueError('source hash mismatch: '+f['path'])
            files[f['path']] = f['sha256']
        sources[source['id']] = files
    ids = set()
    for case in manifest['cases']:
        if case['id'] in ids: raise ValueError('duplicate case')
        ids.add(case['id'])
        files = sources[case['source_id']]
        for side in ('old','new'):
            p,h = case[side+'_path'],case[side+'_sha256']
            if files.get(p) != h or sha(safe_path(root,p).read_bytes()) != h: raise ValueError('case provenance mismatch')
        if case['expected'] is None and case['required']: raise ValueError('unadmitted required case')


def build(data_dir, inventory_path, root):
    import pyarrow.parquet as pq
    root.mkdir(parents=True,exist_ok=True)
    sources=[]; cases=[]
    def write(path, data, source):
        payload=data.encode() if isinstance(data,str) else data
        target=safe_path(root,path);target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(payload)
        source['files'].append({'path':path,'sha256':sha(payload)})
        return path,sha(payload)
    for config,(language,ext) in LANGUAGES.items():
        source={'id':'humanevalpack-'+config,'url':f'https://huggingface.co/datasets/bigcode/humanevalpack/resolve/{REVISION}/{config}/test-00000-of-00001.parquet','revision':REVISION,'license':'MIT','attribution':'BigCode HumanEvalPack authors; see upstream/README.md and per-task records.','files':[],'pilot_viewer_comparison':'Original retained API viewer sample not independently recovered; no equality claim.'}
        parquet=data_dir/config/'test-00000-of-00001.parquet'
        if sha(parquet.read_bytes()) != PARQUET_SHA256[config]: raise ValueError('pinned Parquet hash mismatch: '+config)
        write(f'upstream/{config}/test-00000-of-00001.parquet',parquet.read_bytes(),source)
        rows=pq.read_table(parquet).to_pylist()[:12]
        for index,row in enumerate(rows):
            write(f'upstream/{config}/{index:02}.json',encode(row),source)
            # Keep upstream declarations/imports and bodies verbatim. Go needs its package clause.
            prefix=('package main\n\n'+row['import'] if config=='go' else '')+row['declaration']
            old=prefix+row['buggy_solution'];new=prefix+row['canonical_solution']
            paths={}
            for side,text in [('old',old),('new',new)]:paths[side]=write(f'cases/{config}/{index:02}/{side}.{ext}',text,source)
            common={'language_id':language,'filename':f'fixture.{ext}','source_id':source['id'],'upstream_task_id':row['task_id'],'class':'external_seed_only','required':False,'expected':None,'oracle':{'basis':'Retained upstream repair and tests; semantic-fact mapping not independently admitted.','behavior':'unknown','notes':'HumanEvalPack is a specialist repair dataset; tests retained separately. No engine-output oracle.'}}
            case=dict(common,id=f'hep-{config}-{index:02}-repair',family='repair',old_path=paths['old'][0],new_path=paths['new'][0],old_sha256=paths['old'][1],new_sha256=paths['new'][1]);cases.append(case)
            identity=dict(common,id=f'hep-{config}-{index:02}-identity',family='identity',old_path=paths['new'][0],new_path=paths['new'][0],old_sha256=paths['new'][1],new_sha256=paths['new'][1],**{})
            identity.update({'class':'reviewed_external_derived','required':True,'expected':{'has_semantic_changes':False,'is_style_only':None,'required_changes':[],'forbidden_change_types':['ADDITION','DELETION','MODIFICATION','REFACTORING','MOVE','REORDER']},'oracle':{'basis':'Byte identity: old and new are the identical retained canonical source file.','behavior':'preserved','notes':'Identity establishes no edit, independently of parser/engine output. is_style_only intentionally unconstrained: public API does not define its value on no edit.'}});cases.append(identity)
        sources.append(source)
    # CPP/11 has a retained upstream behavioral witness, independently executed with g++ C++17.
    witness = next(c for c in cases if c['id']=='hep-cpp-11-repair')
    witness.update({'class':'upstream_gold','required':True,'expected':{'has_semantic_changes':True,'is_style_only':False,'required_changes':[{'change_type':'MODIFICATION','old_label':"'1'",'new_label':"'0'"},{'change_type':'MODIFICATION','old_label':"'0'",'new_label':"'1'"}],'forbidden_change_types':[]},'oracle':{'basis':'Original CPP/11 test, retained in upstream/cpp/11.json; g++ -std=c++17 compiled both declaration+solution+test. Old aborts at original first assert, new exits 0.','behavior':'changed','notes':'Witness establishes changed output on original test; exact changed literal payloads independently inspected in original pair. Not universal equivalence proof.'}})
    # Source-grounded literal facts allow any reported changed ancestor containing the exact token.
    # Granularity is not an oracle: the evidence remains two exact literals and their original spans.
    old_cpp=(root/'cases/cpp/11/old.cpp').read_text();new_cpp=(root/'cases/cpp/11/new.cpp').read_text()
    def output_literals(text):
        found=[]
        for match in re.finditer(r"output\+=('(?:0|1)')",text):
            start,end=match.span(1)
            line=text.count('\n',0,start);line_start=text.rfind('\n',0,start)+1
            col=len(text[line_start:start].encode('utf-8'))
            found.append((match.group(1),{'start_line':line,'start_col':col,'end_line':line,'end_col':col+len(match.group(1).encode('utf-8'))}))
        return found
    old_facts=output_literals(old_cpp);new_facts=output_literals(new_cpp)
    if len(old_facts)!=2 or len(new_facts)!=2: raise ValueError('CPP/11 expected two output literals')
    witness['expected']['required_changes']=[{'change_type':'MODIFICATION','match':'descendant_evidence','old_label':old[0],'new_label':new[0],'old_position':old[1],'new_position':new[1]} for old,new in zip(old_facts,new_facts)]
    witness['oracle']['notes']+=' Evidence may appear within changed ancestor nodes: literal-root change granularity is not required. Positions are independent zero-based UTF-8 byte columns, exclusive end.'
    regression={'id':'literal-regressions','url':'https://github.com/buchochelliq-labs/intentumdiff-core/issues/261','revision':'0.0.3-reviewed-literal-regressions-v1','license':'MIT','attribution':'IntentumDiff contributors; independently reviewed C/C++ literal source facts','files':[]}
    for lang in ('c','cpp'):
        examples=[('ordinary',"int f(void) { return '0'; }\n","int f(void) { return '1'; }\n", "'0'", "'1'"),('escape',"int f(void) { return '\\n'; }\n","int f(void) { return '\\t'; }\n", "'\\n'", "'\\t'"),('position',"// é Unicode byte position\n\nint f(void) {\n return 'a';\n}\n","// é Unicode byte position\n\nint f(void) {\n return 'b';\n}\n", "'a'", "'b'")]
        if lang=='cpp': examples.append(('prefix',"auto f() { return 'x'; }\n","auto f() { return L'x'; }\n", "'x'", "L'x'"))
        for name,old,new,old_label,new_label in examples:
            op,oh=write(f'cases/regression/{lang}-{name}-old.{lang}',old,regression);np,nh=write(f'cases/regression/{lang}-{name}-new.{lang}',new,regression)
            cases.append({'id':f'literal-{lang}-{name}','language_id':lang,'filename':f'fixture.{lang}','class':'handwritten_regression','family':'character_literal','old_path':op,'new_path':np,'old_sha256':oh,'new_sha256':nh,'source_id':regression['id'],'upstream_task_id':f'{lang}-{name}','required':True,'oracle':{'basis':'Independent lexical source review: literal payload/type changes; retained complete byte spans include prefix and escapes.','behavior':'changed','notes':'C/C++ character literal value or type changes; no engine output used.'},'expected':{'has_semantic_changes':True,'is_style_only':False,'required_changes':[{'change_type':'MODIFICATION','old_label':old_label,'new_label':new_label}],'forbidden_change_types':[]}})
    for lang in ('c','cpp'):
        original=f"int f(void) {{ return '0'; }}\n"
        for name,new,style in [('identity',original,False),('formatting',"\nint f(void) {\n    return '0';\n}\n",True)]:
            op,oh=write(f'cases/regression/{lang}-{name}-old.{lang}',original,regression);np,nh=write(f'cases/regression/{lang}-{name}-new.{lang}',new,regression)
            cases.append({'id':f'literal-{lang}-{name}','language_id':lang,'filename':f'fixture.{lang}','class':'handwritten_regression','family':'regression_'+name,'old_path':op,'new_path':np,'old_sha256':oh,'new_sha256':nh,'source_id':regression['id'],'upstream_task_id':f'{lang}-{name}','required':True,'oracle':{'basis':'Independent source review: identical source bytes or only whitespace between lexical tokens changed.','behavior':'preserved','notes':'Character literal payload remains exactly unchanged.'},'expected':{'has_semantic_changes':False,'is_style_only':True if style else None,'required_changes':[],'forbidden_change_types':['ADDITION','DELETION','MODIFICATION','REFACTORING','MOVE','REORDER']}})
    sources.append(regression)
    # External-derived reviewed controls use retained Python task zero, preserving its signature.
    python_source=next(s for s in sources if s['id']=='humanevalpack-python')
    original=(root/'cases/python/00/new.py').read_text()
    for name,new,style,required in [('formatting',original.replace('\n','\n\n'),True,[]),('rename',original.replace('has_close_elements','nearby_elements'),False,[{'change_type':'REFACTORING','old_label':'has_close_elements','new_label':'nearby_elements'}])]:
        op,oh=write(f'cases/controls/{name}-old.py',original,python_source);np,nh=write(f'cases/controls/{name}-new.py',new,python_source)
        cases.append({'id':'hep-python-00-'+name,'language_id':'python','filename':'fixture.py','class':'reviewed_external_derived','family':name,'old_path':op,'new_path':np,'old_sha256':oh,'new_sha256':nh,'source_id':python_source['id'],'upstream_task_id':'Python/0','required':True,'oracle':{'basis':'Independent source transformation review: '+('only blank lines inserted; Python lexical statements identical.' if style else 'only public function binding renamed; body and parameters unchanged. Calls must use new name.'),'behavior':'preserved' if style else 'changed','notes':'Rename preserves function body behavior, but changes public binding. Not style-only.'},'expected':{'has_semantic_changes':not style,'is_style_only':style,'required_changes':required,'forbidden_change_types':['ADDITION','DELETION']}})
    for config,(language,ext) in LANGUAGES.items():
        if config=='python': continue
        source=next(s for s in sources if s['id']=='humanevalpack-'+config)
        row=json.loads((root/f'upstream/{config}/00.json').read_text())
        original=(root/f'cases/{config}/00/new.{ext}').read_text()
        entry=row['entry_point'];replacement='nearbyElements' if config in ('js','java') else 'NearbyElements' if config=='go' else 'nearby_elements'
        if len(re.findall(r'\b'+re.escape(entry)+r'\b',original))!=1: raise ValueError('task0 entry binding not unique: '+config)
        for name,new,style,changes in [('formatting','\n'+original,True,[]),('rename',re.sub(r'\b'+re.escape(entry)+r'\b',replacement,original),False,[{'change_type':'REFACTORING','old_label':entry,'new_label':replacement}])]:
            op,oh=write(f'cases/controls/{config}-{name}-old.{ext}',original,source);np,nh=write(f'cases/controls/{config}-{name}-new.{ext}',new,source)
            cases.append({'id':f'hep-{config}-00-'+name,'language_id':language,'filename':f'fixture.{ext}','class':'reviewed_external_derived','family':name,'old_path':op,'new_path':np,'old_sha256':oh,'new_sha256':nh,'source_id':source['id'],'upstream_task_id':row['task_id'],'required':True,'oracle':{'basis':'Independent source delta: '+('one leading blank line; all original bytes follow unchanged.' if style else f'exactly one identifier-token occurrence {entry} replaced with {replacement}; function has no internal reference to old entry binding.'),'behavior':'preserved' if style else 'changed','notes':'Renaming public binding preserves body computation under corresponding call substitution, but changes API; not style-only. Compiler execution not claimed.'},'expected':{'has_semantic_changes':not style,'is_style_only':style,'required_changes':changes,'forbidden_change_types':['ADDITION','DELETION']}})
    metadata={'id' :'human-eval-metadata','url':f'https://huggingface.co/datasets/bigcode/humanevalpack/blob/{REVISION}/README.md','revision':REVISION,'license':'MIT','files':[],'attribution':'BigCode HumanEvalPack authors'}
    write('upstream/README.md',(data_dir/'README.md').read_bytes(),metadata);sources.append(metadata)
    inventory=json.loads(inventory_path.read_text());(root/'inventory.json').write_bytes(encode(inventory))
    table=(root/'upstream/selection-issue-268.md').read_text()
    selected={}
    for line in table.splitlines():
        if not line.startswith('| '):continue
        cols=[s.strip() for s in line.split('|')[1:-1]]
        if len(cols)!=5 or cols[0] not in inventory['parsers']:continue
        url=re.search(r'\]\(([^)]+)\)',cols[1]); selected[cols[0]]=(url.group(1),cols[2],cols[4])
    ledger={'schema_version':1,'languages':[],'inventory':{'path':'inventory.json','sha256':sha((root/'inventory.json').read_bytes()),'url':'https://github.com/buchochelliq-labs/intentumdiff-python/blob/14119103c46f08cedc8b4313b1832c8daa511553/src/intentumdiff/wasm/parser_manifest.json','revision':'14119103c46f08cedc8b4313b1832c8daa511553'}}
    for lang,parser in sorted(inventory['parsers'].items()):
        url,evidence,reason=selected[lang]
        ledger['languages'].append({'language_id':lang,'component':parser['wasm'],'source_url':url,'evidence_class':evidence,'admission_status':'pilot_identity_only' if lang in [v[0] for v in LANGUAGES.values()] else 'blocked','reason':reason+' Pilot repair semantic facts remain unadmitted.' if lang in [v[0] for v in LANGUAGES.values()] else reason,'coverage':{'change_understanding':'gap','parser_fidelity':'not_admitted','robustness':'not_admitted'}})
    manifest={'schema_version':1,'sources':sources,'cases':cases,'declared_counts':{'total':len(cases),'families':dict(sorted(Counter(c['family'] for c in cases).items()))}}
    validate(root,manifest);(root/'manifest.json').write_bytes(encode(manifest));(root/'ledger.json').write_bytes(encode(ledger))

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--data-dir',type=Path);p.add_argument('--inventory',type=Path);p.add_argument('--output',type=Path,default=Path('tests/fixtures/external'));p.add_argument('--validate',action='store_true');a=p.parse_args()
    if a.validate: validate(a.output,json.loads((a.output/'manifest.json').read_text()))
    else:
        if a.data_dir is None or a.inventory is None:p.error('--data-dir and --inventory required for import')
        build(a.data_dir,a.inventory,a.output)
