from pathlib import Path
import argparse,json,statistics,sys
parser=argparse.ArgumentParser(description="Check the default-copy million-record instruction and text budgets.")
parser.add_argument("results",type=Path)
parser.add_argument("--output",type=Path,default=Path("budget-witness.json"))
args=parser.parse_args()
rows=[json.loads(line) for line in args.results.read_text().splitlines()]
checks=[]
for mode in ['clone','frozen','check']:
 for k in [1,4,8]:
  name=f'{mode}-{k}'
  arms={arm:[r for r in rows if r['name']==name and r['arm']==arm] for arm in ['base-workers','change-workers','node']}
  assert all(len(v)>=5 and all(r['parity'] for r in v) for v in arms.values()),name
  med={arm:statistics.median(r['instructions:u'] for r in v) for arm,v in arms.items()}
  text={arm:v[0]['text_bytes'] for arm,v in arms.items()}
  checks.append(dict(name=name, medians=med, text_bytes=text,
                     instruction_budget=med['change-workers']<=2*med['node'],
                     instruction_reduction=med['change-workers']<med['base-workers'],
                     text_reduction=text['change-workers']<text['base-workers']))
p=args.output;p.write_text(json.dumps(checks,indent=2)+'\n')
print(json.dumps(checks,indent=2))
sys.exit(0 if all(r['instruction_budget'] and r['instruction_reduction'] and r['text_reduction'] for r in checks) else 1)
