from pathlib import Path
import subprocess,os,json,sys,time,hashlib
root=Path('/Users/amlug/projects/perry/codex-json-fastpaths')
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts')
compiler=Path('/tmp/perry-json-fastpaths-20260906/perry')
names=['test_gap_json_scanning_boundaries','test_issue_9184_json_parse_strict','test_json','test_json_sso_strings','test_gap_json_advanced','test_gap_json_stringify_replacer_tojson','test_gap_json_prototype_tojson','test_gap_json_replacer_sparse_hole','test_gap_json_array_element_overflow_fields','test_issue_9398_json_pretty_tombstoned_key','test_json_typed_basic','test_json_typed_array','test_json_typed_nested','test_json_typed_mismatch','test_json_lazy_edge_cases','test_json_lazy_predicates','test_json_lazy_iteration','test_json_lazy_indexed','test_json_lazy_reviver','test_json_lazy_per_element']
names += ['test_gap_json_flat_scalar_depth', 'test_gap_json_number_formatting','test_gap_json_wide_cache_lifetime','test_gap_json_unicode_lengths','test_gap_json_small_results','test_gap_json_primitive_arrays','test_gap_json_direct_strings','test_gap_json_direct_objects','test_gap_json_tape_scanning','test_gap_json_parse_scalars','test_gap_json_data_records']
names += ['test_gap_json_empty_object', 'test_gap_json_primitive_object', 'test_gap_json_parse_entry', 'test_gap_json_record_output','test_gap_json_owned_tape']
names += ['test_gap_json_parse_empty', 'test_gap_json_escaped_output']
arm=sys.argv[1]
selected=sys.argv[2:]
if selected:names=selected
env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
env['PERRY_RUNTIME_DIR']=str(art/(arm+'-runtime'))
results=json.loads((art/(arm+'-acceptance.json')).read_text()) if selected else []
results=[r for r in results if r['name'] not in names]
for name in names:
 src=root/'test-files'/(name+'.ts');binary=art/(arm+'-'+name)
 log=art/(arm+'-'+name+'.compile.log');obj=art/(name+'.o')
 if arm in ['changed','number','cache','utf16','unicode','scalar','primitive-array','direct-string','tape-scan','parse-scalar','parse-scalar-cache','data-record','record-array-proof','empty-object-leaf','primitive-object','bounded-preflight','parse-entry','record-output','integer-piece','tape-owned','empty-parse','escaped-output']:
  assert obj.exists(),obj
  c=subprocess.CompletedProcess([],0)
 else:
  with log.open('w') as f:
   c=subprocess.run([str(compiler),str(src),'--no-auto-optimize','--no-cache','--no-link','-o',str(obj)],cwd=compiler.parent,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=180)
 if c.returncode==0:
  with (art/(arm+'-'+name+'.link.log')).open('w') as f:
   c=subprocess.run(['cc',str(obj),str(art/(arm+'-runtime/libperry_runtime.a')),'-lc','-Wl,-dead_strip','-Wl,-no_exported_symbols','-o',str(binary)],stdout=f,stderr=subprocess.STDOUT)
 r={'name':name,'arm':arm,'compile_exit':c.returncode,'object_sha256':hashlib.sha256(obj.read_bytes()).hexdigest() if obj.exists() else None}
 if c.returncode==0:
  for label,cmd in [('node',['/opt/homebrew/bin/node','--experimental-strip-types',str(src)]),('perry',[str(binary)])]:
   try:
    p=subprocess.run(cmd,env=env,capture_output=True,text=True,timeout=120)
    r[label]={'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
   except subprocess.TimeoutExpired:r[label]={'exit':'timeout'}
  r['matches_node']=r['node']['exit']==0 and r['perry']['exit']==0 and r['node']['stdout']==r['perry']['stdout']
 results.append(r)
 (art/(arm+'-acceptance.json')).write_text(json.dumps(results,indent=2)+'\n')
 print(arm,name,r.get('matches_node',False),flush=True)
print('FINISHED',arm,len(results),'matched',sum(r.get('matches_node',False) for r in results),flush=True)

assert all(r.get('matches_node',False) for r in results), 'Acceptance failure'
