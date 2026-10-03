"""Diagnostic arena occupancy across existing natural synchronous full cycles.

Arms the existing census at its existing mark-complete hook. Never requests an
extra collection or changes workload source. Census allocations/debugger stops
perturb RSS and timing; this is explicitly not peak-RSS acceptance evidence.
RSS before arming/reporting is retained separately from census-reported RSS.
"""
import pathlib, json, hashlib, subprocess, struct, re
import bench
B = bench.B
Q = B / 'qb6-currentc170-build-v22'
V = 'gc-currentc170-natural-arena-v26'
O = B / V
O.mkdir(exist_ok=True)
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
oracle = json.loads((B / 'gc-currentc170-qb6-v22-noauto-oracle.json').read_text())
builds = json.loads((B / 'gc-currentc170-qb6-v22-noauto-builds.json').read_text())
records = []
for arm in ['currentc170-base', 'currentc170-gc']:
    for case, args in [('tscwork', ['1']), ('zodwork', ['200'])]:
        binary = Q / 'bin' / f'gc-qb6-currentc170-v22-noauto-{arm}-{case}'
        matches = [r for r in builds if r['arm'] == arm and r['case'] == case]
        assert len(matches) == 1 and sha(binary) == matches[0]['binary_sha256']
        symbols = [x.split() for x in subprocess.check_output(['nm', '-n', binary], text=True).splitlines()]
        def find(needle):
            hits = [x for x in symbols if len(x) == 3 and x[2].split('.llvm.')[0].endswith(needle)]
            assert len(hits) == 1, (needle, hits)
            return int(hits[0][0], 16), hits[0][2]
        arm_addr, arm_symbol = find('6census10census_arm')
        hook_addr, hook_symbol = find('6census21census_pass1_if_armed')
        _, take_symbol = find('6census11take_census')
        image = binary.read_bytes()
        phoff = struct.unpack_from('<Q', image, 32)[0]
        phentsize, phnum = struct.unpack_from('<HH', image, 54)
        label = None
        for i in range(phnum):
            typ, flags, off, va, pa, filesz, memsz, align = struct.unpack_from('<IIQQQQQQ', image, phoff+i*phentsize)
            if typ == 1 and flags == 4:
                pos = image.find(b'manual', off, off+filesz)
                if pos >= 0: label = va+pos-off; break
        assert label is not None
        prefix = f'{V}-{arm}-{case}'
        census, captures = O/(prefix+'.jsonl'), O/(prefix+'.boundaries.jsonl')
        out, err, script = O/(prefix+'.app.out'), O/(prefix+'.app.err'), O/(prefix+'.gdb')
        assert not census.exists() and not captures.exists()
        observer = O/(prefix+'.observer.py')
        observer.write_text('''import gdb,json,pathlib
CAPTURES=pathlib.Path(%r)
def capture(phase):
 pid=gdb.selected_inferior().pid
 proc=pathlib.Path('/proc')/str(pid)
 row=dict(phase=phase,full=int(gdb.parse_and_eval('$fulls')),smaps_rollup=(proc/'smaps_rollup').read_text(),status=(proc/'status').read_text())
 with CAPTURES.open('a') as f:f.write(json.dumps(row)+'\\n')
class BeforeReport(gdb.Breakpoint):
 def stop(self):capture('before_report');return False
BeforeReport('*'+%r)
''' % (str(captures), take_symbol))
        script.write_text(f'''set pagination off
set confirm off
set language c
set disable-randomization on
set environment PERRY_GC_CENSUS {census}
set environment MIMALLOC_ALLOW_THP 0
set $fulls = 0
source {observer}
break *{hook_symbol}
commands
silent
set $fulls = $fulls + 1
python capture('mark_complete_before_arm')
set $base = $pc - {hook_addr}
call ((void (*)(const char *, unsigned long))($base + {arm_addr}))((const char *)($base + {label}), 6)
continue
end
run {' '.join(args)} > {out} 2> {err}
printf "NATURAL_FULLS=%d\\n", $fulls
quit
''')
        r = bench.run(prefix, ['gdb', '-q', '--batch', '-x', script, binary], timeout=900)
        assert r['rc'] == 0 and r['reason'] is None and out.read_text() == oracle[case], r
        rows = [json.loads(x) for x in census.read_text().splitlines()] if census.exists() else []
        bounds = [json.loads(x) for x in captures.read_text().splitlines()] if captures.exists() else []
        full_counts = re.findall(r'^NATURAL_FULLS=(\d+)$', r['stdout'], re.M)
        assert len(full_counts) == 1 and int(full_counts[0]) == len(rows)
        assert len(bounds) == 2*len(rows)
        for i,row in enumerate(rows):
            assert row['seq'] == i and row['totals']['reachability_pass']
            assert bounds[2*i]['phase'] == 'mark_complete_before_arm' and bounds[2*i+1]['phase'] == 'before_report'
            assert bounds[2*i]['full'] == bounds[2*i+1]['full'] == i+1
            spaces = row['arena']['spaces'][:5]
            assert sum(x['capacity_bytes'] for x in spaces) == row['arena']['capacity_bytes']
            assert sum(x['used_bytes'] for x in spaces) == row['arena']['used_bytes']
        r.update(arm=arm,case=case,binary_sha256=sha(binary),script_sha256=sha(script),observer_sha256=sha(observer),natural_synchronous_fulls=len(rows),correct=True,census_sha256=sha(census) if rows else None,boundaries_sha256=sha(captures) if bounds else None)
        records.append(r); bench.m.save(V+'-runs.json', records)
        print(arm,case,'natural fulls',len(rows),flush=True)
(B/(V+'.exit')).write_text('0\n')
