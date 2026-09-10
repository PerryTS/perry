from pathlib import Path
import json
w=Path(__file__).resolve().parent
selected={('records_object_1m','parse'),('records_array_20m','scan')}
rows=json.loads((w/'screen-cases.json').read_text())['cases']
cases=[]
for row in rows:
 if (row['fixture'],row['operation']) in selected:
  cases.append(row|{'iterations':row['iterations']*4,'repetitions':11})
assert len(cases)==2
(w/'recheck-cases.json').write_text(json.dumps({'cases':cases,'selection':'Two separated R15 screen slowdowns, using the same two cases and longer work counts as the flat R14 identical-main A/A controls. Four times original work, eleven repetitions, unchanged warmup. Selected after the screen, before recheck timing.'},indent=2)+'\n')
s=(w/'measure-screen.py').read_text().replace("'screen-cases.json'","'recheck-cases.json'").replace('len(cases) == 12 and all(c[\'repetitions\'] == 7','len(cases) == 2 and all(c[\'repetitions\'] == 11').replace('-screen-focus','-recheck-focus')
(w/'measure-recheck.py').write_text(s)
s=(w/'analyze-screen.py').read_text().replace('repetitions = 7','repetitions = 11').replace("'-screen-'","'-recheck-'").replace("'focus': 12","'focus': 2").replace('total == 336','total == 88').replace("'screen-analysis.json'","'recheck-analysis.json'")
(w/'analyze-recheck.py').write_text(s)
for name in ['measure-recheck.py','analyze-recheck.py']:compile((w/name).read_text(),name,'exec')
print('Prepared two cases, 88 timed trials, 10 complete-output checks; no new remote staging needed.')
