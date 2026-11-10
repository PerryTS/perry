#!/usr/bin/env python3
"""Print the leak count of a finished census analysis: birth nodes in total
and per injection kind (kinds overlap: a node with several independent
injectors counts under each). Reads PERRY_BIRTH_OUTPUT/birth-summary.json
(or the directory given as argv[1]). Output is TSV, sorted by count then name."""
import json,os,sys
from pathlib import Path
R=Path(sys.argv[1] if len(sys.argv)>1 else os.environ.get('PERRY_BIRTH_OUTPUT','.'))
s=json.load(open(R/'birth-summary.json'))
print('kind\tbirth_nodes')
print(f"TOTAL\t{s['births']}")
print(f"injector_locations\t{s['independent_injection_locations']}")
for k,v in sorted(s['kind_births'].items(),key=lambda kv:(-kv[1],kv[0])):print(f'{k}\t{v}')
