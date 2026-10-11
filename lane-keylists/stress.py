#!/usr/bin/env python3
"""Check the lane's existing binaries under moving-GC verification."""
import argparse, json, re
import run as harness
p=argparse.ArgumentParser();p.add_argument('arm',default='head',nargs='?');p.add_argument('--only',nargs='+');a=p.parse_args()
records=[]
for name,(directory,source,args) in harness.PROGRAMS.items():
 if a.only and name not in a.only:continue
 out=harness.EVIDENCE/a.arm
 _,env=harness.arm_env(a.arm)
 env.update(PERRY_GC_FORCE_EVACUATE='1',PERRY_GC_VERIFY_EVACUATION='1',PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_FROMSPACE_SCAN='1',PERRY_GC_SCHEDULE_SEED='1')
 result=harness.run(['taskset','-c',harness.CPUS,str(out/(name+'.observed')),*args],harness.ROOT/'drivers'/directory,env,out/(name+'.stress'))
 oracle=(out/(name+'.node.out')).read_bytes()
 row=dict(name=name,rc=result.returncode,equal=harness.normalized(result.stdout)==harness.normalized(oracle))
 counters=re.search(rb'\[keylists-observe\] ([^\n]+)',result.stderr)
 if counters:row.update({k.decode():int(v) for k,v in re.findall(rb'(\w+)=(\d+)',counters[1])})
 records.append(row)
 (harness.EVIDENCE/'stress.json').write_text(json.dumps(records,indent=2)+'\n')
 print(row,flush=True)
 if result.returncode or not row['equal']:raise RuntimeError(row)
