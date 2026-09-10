from pathlib import Path
import json,math,sys
root=Path(sys.argv[1])
verified=[json.loads(line) for line in (root/'verify.jsonl').read_text().splitlines()]
units={(r['fixture'],r['operation']):r['checksum'] for r in verified if r['engine']=='node'}
checked={}
for phase in ['verify','calibration','timing','memory']:
    rows=[json.loads(line) for line in (root/(phase+'.jsonl')).read_text().splitlines()]
    for r in rows:
        assert not r.get('error') and r['exit_code']==0,(phase,r)
        assert all(math.isfinite(r[k]) for k in ['checksum','retained','rss_before','rss_after','peak_rss','wall_ms','user_us','system_us']),(phase,r)
        operation=r['operation'].removeprefix('retain-')
        calls=r['iterations']+r['warmup']
        expected=units[r['fixture'],operation]*calls
        assert expected.is_integer() and abs(expected)<=2**53,(phase,r)
        assert r['checksum']==expected,(phase,r,expected)
        assert r['retained']==(calls if r['operation'].startswith('retain-') else 0),(phase,r)
    checked[phase]=len(rows)
(root/'checksum-validation.json').write_text(json.dumps({'checked':checked,'all_finite':True,'all_checksums_match_node_per_call':True,'retained_counts_match':True},indent=2)+'\n')
print('PASS original checksums and retained counts:',checked)
