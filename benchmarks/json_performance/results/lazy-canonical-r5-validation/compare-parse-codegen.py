from pathlib import Path
import json,re
base=Path(__file__).resolve().parent
d=json.loads((base/'tape-codegen.json').read_text())
out={}
for left,right in zip(d['lazy-canonical-r4']['functions'],d['lazy-canonical-r5']['functions']):
    assert left['name']==right['name']
    def instructions(f):
        return [(m[1],m[2],m[3]) for line in f['assembly'].splitlines()
                if (m:=re.match(r'^([0-9a-f]+):\s+([0-9a-f]{8})\s+(.*)$',line))]
    a,b=instructions(left),instructions(right)
    assert a and b
    differences=[{'index':i,'r4':x,'r5':y} for i,(x,y) in enumerate(zip(a,b)) if x!=y]
    out[left['name']]={'r4_instructions':len(a),'r5_instructions':len(b),
                      'identical_instructions':a==b,'differences':differences}
(base/'parse-instruction-comparison.json').write_text(json.dumps(out,indent=2)+'\n')
assert all(x['identical_instructions'] for x in out.values())
print('PASS',sum(x['r4_instructions'] for x in out.values()),'captured parse instruction rows identical')
