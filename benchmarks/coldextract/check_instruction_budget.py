"""Corpus-specific performance witness; not a runtime policy or dispatch rule."""
import json,sys
from pathlib import Path
root=Path('/root/lanes/perry-coldextract')
stats=json.loads((root/'summary/statistics.json').read_text())['micro-inst']
arm=sys.argv[1]
# Fixed complete corpus and pass counts from driver.ts. Each allowance is
# deliberately between original-main and fixed measurements, above Node.
budgets={'hash':2_900_000_000,'file-hash':7_700_000_000,'verify':2_900_000_000}
failed=[]
for name,limit in budgets.items():
 row=stats[name][arm]['instructions:u']
 assert row['n']==5, (name, 'requires five interleaved runs')
 print(name,arm,row['median'],limit,'PASS' if row['median']<=limit else 'FAIL')
 if row['median']>limit:failed.append(name)
assert not failed, ('wide digest instruction budget',failed)
