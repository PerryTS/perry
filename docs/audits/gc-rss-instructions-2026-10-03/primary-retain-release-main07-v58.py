"""Fresh, interleaved RSS repeats on the exact three V46 binaries."""
import pathlib, json, hashlib
import bench
B = bench.B
V = 'gc-retain-release-main07-v58'
PRIOR = 'gc-auto-entry-once-main07-v46'
def sha(p):
    with pathlib.Path(p).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()
assert (B/(PRIOR+'-driver.exit')).read_text().strip() == '0'
assert not (B/(V+'-runs.json')).exists()
prior = json.loads((B/(PRIOR+'-runs.json')).read_text())
builds = json.loads((B/(PRIOR+'-builds.json')).read_text())
case = '73-retain-then-release'
arms = ['gc', 'entry', 'once']
binaries = {}
for arm in arms:
    rows = [r for r in prior if r['case'] == case and r['arm'] == arm and r['mode'] == 'plain']
    assert len(rows) == 3 and all(r['correct'] for r in rows)
    binary = pathlib.Path(rows[0]['cmd'][3])
    assert all(r['binary_sha256'] == sha(binary) for r in rows)
    build = next(r for r in builds if r['arm'] == arm and pathlib.Path(r['source']).stem == case)
    assert build['binary_sha256'] == sha(binary)
    binaries[arm] = str(binary)
src = B/'sources'/(case+'.ts')
assert all(r['source_sha256'] == sha(src) for r in builds if pathlib.Path(r['source']).stem == case)
oracle = bench.run(V+'-oracle', ['node', '--expose-gc', src], timeout=300)
assert oracle['rc'] == 0 and oracle['reason'] is None
assert oracle['stdout'] == json.loads((B/(PRIOR+'-oracle.json')).read_text())[case]
bench.m.save(V+'-inputs.json', dict(prior=PRIOR, prior_verification_sha256=sha(B/(PRIOR+'-verification.json')),
    prior_runs_sha256=sha(B/(PRIOR+'-runs.json')), prior_builds_sha256=sha(B/(PRIOR+'-builds.json')),
    source=str(src), source_sha256=sha(src), binaries={a:dict(path=p, sha256=sha(p)) for a,p in binaries.items()},
    oracle=oracle['stdout'], scope='RSS repeats only; exact V46 binaries, default GC, no rebuild or policy change.'))
rows = []
for rep in range(9):
    order = arms[rep%3:] + arms[:rep%3]
    for arm in order:
        binary = binaries[arm]
        r = bench.run(f'{V}-plain-{arm}-{rep}', ['/usr/bin/time', '-f', 'RSS_KIB=%M WALL=%e USER=%U SYS=%S', binary], timeout=300)
        r.update(case=case, arm=arm, repeat=rep, binary_sha256=sha(binary), correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracle['stdout'])
        rows.append(r)
        bench.m.save(V+'-runs.json', rows)
        assert r['correct'], r
        print(arm, rep, r['peak_rss_bytes'], flush=True)
assert len(rows) == 27
(B/(V+'.exit')).write_text('0\n')
