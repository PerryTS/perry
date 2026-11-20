import csv,json,collections,sys
from pathlib import Path
S=Path(sys.argv[1])
for f in ['allinputs','r1','r2']:
 d=S/'fx'/f/'head'
 r=list(csv.DictReader(open(d/'report.inputs.tsv'),delimiter='\t'))
 assert all(int(a['seq'])<int(b['seq']) for a,b in zip(r,r[1:]))
 assert all(int(a['ir'])>0 for a in r)
 n=[json.loads(x) for x in open(d/'native.jsonl')]
 assert n[-1]['kind']=='summary' and n[-1]['main_rc']==0,n[-1]
 assert not n[-1]['memory_failures'],n[-1]
 unknown=[(x.get('nr'),x.get('name'),x.get('args')) for x in n if x.get('complete') is False]
 assert not unknown,unknown
 markers={int(x['runtime_seq']) for x in n if x['kind']=='runtime_marker'}
 ordinary={int(x['seq']) for x in r if x['op']!='first_changed_tracked_word'}
 assert ordinary<=markers,(ordinary-markers)
 ops=collections.Counter(x['op'] for x in r)
 assert any(x["kind"]=="guard_enter" for x in n),"guard entry probe never ran"
 assert any(x["kind"]=="guard_complete" for x in n),"completed-module boundary probe never ran"
 print(f,'events',len(r),'markers',len(markers),'native',len(n),'complete=True')
 if f=='allinputs':
  required=['initial_environment','getenv','env_get','env_has','env_block','argv','cwd','locale.navigator',
   'locale.time_zone','tty.isatty','ids.uid','ids.gid','ids.pid','ids.ppid','clock.wall_millis',
   'clock.performance','clock.hrtime_bigint','random.Math','random.crypto.bytes','random.crypto.uuid',
   'crypto.random.bytes','stack.capture','stack.payload','stack.rendered','native.clock_gettime']
  assert not(set(required)-ops.keys()),set(required)-ops.keys()
  assert any(x.get('name') in ('read','pread64') and x.get('bytes') for x in n)
  assert any(x.get('name')=='getrandom' and x.get('bytes') for x in n)
  assert any(x.get('is_error') for x in n)
  print('all required scalar/byte host classes observed with values')
 if f=='r1':
  inputs=[x for x in r if x['op']=='env_get']
  assert inputs
  assert any(any(int(p.split(':')[1])==1 for p in x['stack_parts'].split(',') if p) for x in inputs)
  print('R1 parent dependency-phase flag observed before child input')
 if f=='r2':
  first_write=[x for x in r if x['op']=='first_changed_tracked_word']
  enters=[x for x in r if x['op']=='init_enter']
  assert any(int(w['seq'])<int(e['seq']) for w in first_write for e in enters),[(x['seq'],x['init']) for x in first_write]
  assert '11' in (d/'stdout').read_text(),(d/'stdout').read_text()
  print('non-resumable parent write observed before nested child; normal final state=11')
print('GREEN')

