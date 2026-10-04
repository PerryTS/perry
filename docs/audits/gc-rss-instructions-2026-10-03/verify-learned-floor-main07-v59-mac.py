from pathlib import Path
import json, hashlib, re
B=Path('/Users/amlug/projects/perry/secret-tests/scratchpad/rss-header-20261002')
W=Path('/Users/amlug/projects/perry/rss-gc-learned-floor-main07-20261004')
V='learned-floor-main07-v59'
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
m=json.loads((B/(V+'-inputs.json')).read_text())
r=json.loads((B/(V+'-mac-result.json')).read_text())
assert len(m)==5011 and all(sha(W/n)==h for n,h in m.items())
assert r['rc']==0 and r['raw_log_sha256']==sha(B/(V+'-mac-runtime.log'))
assert r['source_manifest_sha256']==sha(B/(V+'-inputs.json'))
s=(B/(V+'-mac-runtime.log')).read_text()
assert 'Compiling perry-runtime ' in s
counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; 0 measured; (\d+) filtered out;',s)
assert counts[-1]==('4879','0','5','0')
assert len(counts[:-1])==8 and all(c==('1','0','0','4883') for c in counts[:-1])
for n in ['lower_nursery_cap_requires_sustained_low_influx_and_preserves_lifetimes','multi_step_moves_land_in_the_dead_band_and_hold','cap_scale_grows_on_heavy_influx_and_shrinks_when_quiet','allocation_census','successful_copying_minor_does_not_query_fallback_rss','pinned_minor_fallback_still_samples_rss_and_preserves_root']:
    assert re.search(r'test .*'+n+r'.* \.\.\. ok',s),n
p=dict(private_commit='3fb6cd50ede80fe2cbef031d984f70c9dcb13def',parent_private_commit='d4f491673b1d5930eaaffdd1f03c939754091eb5',source_count=len(m),runtime=dict(passed=4879,failed=0,ignored=5,filtered=0),isolated_subprocess_tests=8,result=r,scope='Private learned floor, full unfiltered Mac runtime tests, unchanged source inputs. First proof mistakenly expected subprocess filtered count zero; subprocesses intentionally select one test. Second proof attempt used an incorrect relative artifact path. Application RSS and instructions unproven.')
(B/(V+'-mac-verification.json')).write_text(json.dumps(p,indent=2)+'\n')
print(json.dumps(p))
