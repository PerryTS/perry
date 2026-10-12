#!/usr/bin/env python3
"""Exact Node parity and protected moving-GC verification for allocation edges."""
import argparse
import importlib.util
import json
from pathlib import Path
import re

spec=importlib.util.spec_from_file_location('common',Path(__file__).resolve().parents[2]/'scripts/perf_objops_lane.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
p=argparse.ArgumentParser();p.add_argument('--hostdir',required=True,type=Path);p.add_argument('--arms',default='main,head');a=p.parse_args()
root=a.hostdir
cases=['test_gap_allocstate_birth','test_gap_class_birth_f64','test_gap_small_cold_class_bump_birth',
       'test_gap_gc_new_instance_rooting','test_gap_gc_inline_ctor_this_rooting',
       'test_gap_11420_literal_birth_shape','test_gap_object_spread_shape_birth',
       'test_gap_gc_staging_args_rooting','test_gap_gc_coalesce_local_root']
status={}
for name in cases:
    source=root/'src/test-files'/f'{name}.ts'
    expected=m.oracle(root,name,source,[],root/'src')
    status[name]={}
    for arm in a.arms.split(','):
        result=m.compile_one(root,arm,name,source,root/'src',expected,[])
        status[name][arm]=result
        if result['status']=='pass':
            env=m.environment(root,arm)
            env.update(PERRY_GC_FORCE_EVACUATE='1',PERRY_GC_VERIFY_EVACUATION='1',
                       PERRY_GC_PROTECT_FROMSPACE='1',PERRY_GC_SCHEDULE_SEED='129',
                       PERRY_GC_SCHEDULE_RATE='0.2',PERRY_GC_DIAG='1')
            run=m.execute([str(root/'measure'/arm/name)],root/'src',env,
                          root/'measure'/arm/f'{name}.stress',allow_fail=True)
            result['stress']='pass' if run.returncode==0 and run.stdout==expected else 'fail'
            result['retired_sets']=run.stderr.count(b'retired_set=#')
            result['stress_exit']=run.returncode
            result['copied_objects']=sum(int(n) for n in re.findall(rb'copied_objects=(\d+)',run.stderr))
        print(name,arm,result,flush=True)
    (root/'measure/gaps.json').write_text(json.dumps(status,indent=2))
