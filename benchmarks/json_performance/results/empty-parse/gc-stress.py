from pathlib import Path
import os,subprocess,json,re
art=Path('/Users/amlug/projects/perry/codex-json-fastpaths-artifacts');arm='empty-parse'
accept={r['name']:r for r in json.loads((art/(arm+'-acceptance.json')).read_text())}
names=['test_gap_json_scanning_boundaries','test_gap_json_wide_cache_lifetime','test_gap_json_unicode_lengths','test_gap_json_small_results','test_gap_json_primitive_arrays','test_gap_json_direct_strings','test_gap_json_direct_objects','test_gap_json_tape_scanning','test_gap_json_parse_scalars','test_gap_json_data_records']
names += ['test_gap_json_empty_object', 'test_gap_json_primitive_object', 'test_gap_json_parse_entry', 'test_gap_json_record_output','test_gap_json_owned_tape']
names += ['test_gap_json_parse_empty']
rows=[]
for name in names:
 for seed in [17,9013]:
  env={k:v for k,v in os.environ.items() if not k.startswith('PERRY_')}
  env.update(PERRY_GC_SCHEDULE_SEED=str(seed),PERRY_GC_SCHEDULE_RATE='0.1',PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_VERIFY_EVACUATION='1')
  p=subprocess.run([str(art/(arm+'-'+name))],env=env,capture_output=True,text=True,timeout=180)
  counts={k:int(v) for k,v in re.findall(r'(scheduled_collections|copying_minors|moved_objects|loop_polls)=(\d+)',p.stderr)}
  row={'fixture':name,'seed':seed,'exit':p.returncode,'stdout':p.stdout,'stderr':p.stderr,'counters':counts,'matches_node':p.returncode==0 and p.stdout==accept[name]['node']['stdout'],'live_subject':all(counts.get(k,0)>0 for k in ['scheduled_collections','copying_minors','moved_objects','loop_polls'])}
  rows.append(row);(art/(arm+'-all-gc-stress.json')).write_text(json.dumps(rows,indent=2)+'\n')
  print(name,seed,row['matches_node'],row['live_subject'],counts,flush=True)
  assert row['matches_node'] and row['live_subject'],row
