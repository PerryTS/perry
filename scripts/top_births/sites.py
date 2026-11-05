#!/usr/bin/env python3
"""Extract the exact identity set for one birth from the compressed set ledger."""
import argparse,csv,gzip,os,sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('birth',type=int);p.add_argument('--raw',action='store_true');p.add_argument('--sole',action='store_true');a=p.parse_args()
path=Path(os.environ.get('PERRY_BIRTH_OUTPUT', '.'))/(('raw-' if a.raw else '')+'birth-sites.tsv.gz')
mask_path=path.with_name(('raw-' if a.raw else '')+'birth-masks.tsv.gz')
selected=set()
for r in csv.DictReader(gzip.open(mask_path,'rt'),delimiter='\t'):
 if int(r['births_hex'],16)&(1<<a.birth):selected.add(r['mask'])
w=csv.writer(sys.stdout,delimiter='\t');w.writerow(['file','start','end','kind','key'])
for r in csv.DictReader(gzip.open(path,'rt'),delimiter='\t'):
 if r['mask'] in selected and (not a.sole or r['sole']=='1'):
  w.writerow([r[k] for k in ['file','start','end','kind','key']])
