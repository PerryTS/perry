#!/usr/bin/env python3
"""IR-shape witness for the guarded counter; run with a freshly built perry."""
import argparse, os, re, subprocess, tempfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--perry',required=True);args=p.parse_args()
repo=Path(__file__).resolve().parents[1]
source=repo/'test-files/test_gap_byte_scanning_bounded_counter.ts'
with tempfile.TemporaryDirectory(prefix='perry-bounded-counter-') as td:
 d=Path(td);target=d/source.name;target.write_bytes(source.read_bytes())
 c=subprocess.run([str(Path(args.perry).resolve()),'compile',str(target),'--no-cache','--trace','llvm','-o',str(d/'witness')],cwd=d,capture_output=True,text=True)
 assert c.returncode==0,c.stderr[-4000:]
 assert 'falling back' not in (c.stdout+c.stderr).lower()
 ir='\n'.join(f.read_text() for f in (d/'.perry-trace/llvm').glob('*.ll'))
 bodies=re.findall(r'^define [^\n]*@[^\s(]*scanBounded[^\s(]*\([^\n]*\{\n.*?^\}',ir,re.M|re.S)
 assert bodies,'missing bounded-counter function'
 fast_conditions=[]
 for b in bodies:
  fast_conditions += re.findall(r'^for\.number_locals_fast\.cond[^:]*:\n(.*?)(?=^[^\s:]+:|\Z)',b,re.M|re.S)
 assert any('icmp slt i32' in c for c in fast_conditions), 'bounded scanner must compare its canonical i32 counter'
 read_guards=[g for b in bodies for g in re.findall(r'^ta\.read\.guard[^:]*:\n(.*?)(?=^[^\s:]+:|\Z)',b,re.M|re.S)]
 assert any('fcmp' not in g for g in read_guards), 'canonical nonnegative i32 slot must discharge floating key checks'
 assert any('2147483646.0' in b for b in bodies), 'entry guard must reserve the body increment before the outer ++'
 assert any('9223372036854775808' in b for b in bodies), 'entry guard must preserve observable negative zero on a miss'
 assert any('load volatile i32' in b and '@js_gc_loop_safepoint' in b for b in bodies), 'integer representation must keep the armed back-edge poll'
 offsets=re.findall(r'^define [^\n]*@[^\s(]*(?:scanOffset|scanStep)[^\s(]*\([^\n]*\{\n.*?^\}',ir,re.M|re.S)
 assert offsets, 'missing computed-index scanner'
 for b in offsets:
  chunks=re.split(r'^([^\s:]+):\n',b,flags=re.M)
  blocks={chunks[j]:chunks[j+1] for j in range(1,len(chunks),2)}
  queue=[label for label in blocks if label.startswith('for.number_locals.fast.preheader.')];seen=set()
  while queue:
   label=queue.pop()
   if label in seen or label.startswith('for.number_locals.merge.'):continue
   seen.add(label);queue += re.findall(r'label %([^,\s]+)',blocks.get(label,''))
  fast='\n'.join(blocks[label] for label in blocks if label in seen)
  if seen:
   assert 'load volatile i32' in fast and '@js_gc_loop_safepoint' in fast, 'computed scanner index must retain its armed back-edge poll'
 print('bounded byte counter IR: PASS')
