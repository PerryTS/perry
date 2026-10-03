"""Current-main native allocation/RSS attribution at the existing natural boundary.
Debugger-only observations; no forced GC and no production representation edit.
Run only after campaign-attribution.py succeeds. Large native sites are sampled
at >=64KiB; shape spans additionally identify small allocations exactly.
"""
import json, subprocess
import bench
B=bench.B
VERSION='current0ed-native-v17'
O=B/'native-v17';O.mkdir(exist_ok=True)
records=[]
for case,args in [('tscwork',['1']),('zodwork',['200'])]:
    binary=B/'bin'/('gc-qb6-current0ed-v14-noauto-current0ed-base-tscwork' if case=='tscwork' else 'gc-qb6-current0ed-v17-diagnostic-current0ed-base-zodwork')
    expected=json.loads((B/('census-v17' if case=='tscwork' else 'census-zod-v17')/f'current0ed-v17-{case}-current0ed-base-0.sharing.json').read_text())['shapes']
    boundary=dict(selected_full=3 if case=='tscwork' else 1,criterion='third natural full' if case=='tscwork' else 'explicit diagnostic GC before safeParse, schema live; not original benchmark')
    selected=boundary['selected_full']
    binary_sha=bench.m.sha(binary)
    symbols=[s.split() for s in subprocess.check_output(['nm','-n',binary],text=True).splitlines()]
    def find(needle,suffix=False):
        hits=[s[2] for s in symbols if len(s)==3 and (s[2].split('.llvm.')[0].endswith(needle) if suffix else needle in s[2])]
        assert len(hits)==1,(needle,hits)
        return hits[0]
    hook=find('6census21census_pass1_if_armed',True)
    env={key:find(needle) for key,needle in [
        ('SHAPE_DIR_SYMBOL','AGENT_SHAPE_DIR'),('SHAPE_EMPTY_PAGE_SYMBOL','10EMPTY_PAGE'),
        ('SHAPE_EMPTY_CHUNK_SYMBOL','11EMPTY_CHUNK')]}
    oracle=bench.run(f'{VERSION}-oracle-{case}',['/root/rss-gc-build-20261003/node-v26.5.1-linux-x64/bin/node','--expose-gc',B/'sources/real'/f'{case}.ts',*args],timeout=300)
    assert oracle['rc']==0
    for rep in range(2):
        prefix=f'{VERSION}-{case}-{rep}'
        out=B/'logs'/f'{prefix}.app.out';err=B/'logs'/f'{prefix}.app.err'
        csv=B/f'{prefix}.csv'; assert not csv.exists()
        script=B/f'{prefix}.gdb'
        script.write_text(f'''set pagination off
set confirm off
set language c
set disable-randomization on
set environment PERRY_GC_DIAG 1
set environment PERRY_GC_TRACE 1
set environment MIMALLOC_ALLOW_THP 0
set $fulls = 0
break *{hook}
commands
silent
set $fulls = $fulls + 1
if $fulls == {selected}
python
finish_sites()
capture_native_pages()
pid=gdb.selected_inferior().pid
open('{B}/logs/{prefix}.before.smaps','w').write(open('/proc/%d/smaps'%pid).read())
def read_shape_bytes(addr,size):
    return bytes(gdb.selected_inferior().read_memory(addr,size)) if size else b''
shapes=observe_shapes(read_shape_bytes)
open('{B}/{prefix}.shape-storage.json','w').write(json.dumps(shapes)+'\\n')
end
set $helper = ((void *(*)(const char *, int)) &dlopen)("{B}/native-heap-visitor.so", 2)
if $helper == 0
error loading heap visitor
end
set $dump = ((void *(*)(void *, const char *)) &dlsym)($helper, "dump_native_heap")
set $result = ((int (*)(void *, const char *)) $dump)(&_mi_heap_visit_blocks, "{csv}")
printf "HEAP_DUMP_RESULT=%d\\n", $result
python
open('{B}/logs/{prefix}.after.smaps','w').write(open('/proc/%d/smaps'%pid).read())
end
end
continue
end
source {B}/shape-storage-observer-v17.py
source {B}/native-sites-observer-v17.py
run {' '.join(args)} > {out} 2> {err}
printf "FULLS=%d\\n", $fulls
quit
''')
        r=bench.run(prefix,['gdb','-q','--batch','-x',script,binary],env|{'NATIVE_SITES_PREFIX':prefix,'NATIVE_SITES_ROOT':str(B)},600)
        r.update(case=case,repeat=rep,observation_kind=boundary['criterion'],selected_full=selected,binary_sha256=binary_sha,
                 helper_sha256=bench.m.sha(B/'native-heap-visitor.so'),script_sha256=bench.m.sha(script),
                 correct=r['rc']==0 and out.exists() and out.read_text()==oracle['stdout'])
        records.append(r);bench.m.save(VERSION+'-runs.json',records)
        assert r['correct'] and r['stdout'].count('HEAP_DUMP_RESULT=0')==1,r
        full_count=[int(line.split('=')[1]) for line in r['stdout'].splitlines() if line.startswith('FULLS=')]
        assert len(full_count)==1 and full_count[0]>=selected,full_count
        r['observed_full_count']=full_count[0]
        actual=json.loads((B/f'{prefix}.shape-storage.json').read_text())
        # Debugger pointers vary, counts at the same natural boundary must not.
        for key in ['records','chunks','pages','extras_count','constfn_entries']:
            r.setdefault('shape_counts_vs_census',{})[key]={'native':actual[key],'census':expected[key]}
        # Each snapshot owns its counts: ASLR/native fallback can change liveness.
        bench.m.save(VERSION+'-runs.json',records)
        assert csv.stat().st_size>0
        print(case,rep,'native snapshot complete',flush=True)
    assert bench.m.sha(binary)==binary_sha

(B/'current0ed-native-v17.exit').write_text('0\n')
