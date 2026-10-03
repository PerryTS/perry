"""Linked default and genuinely moving GC checks; exact Node output on both arms."""
import json
import bench
B=bench.B; V='gc-current334-v10-linked'; records=[]
root=B/'perry-current334-gc'
arms=['current334-base','current334-gc']
cases=['test_gap_gc_container_value_rooting','test_gap_gc_define_properties_key_rooting','test_gap_gc_store_ic_old_to_young','test_gap_class_accessor_shape_facts','test_gap_10753_dynamic_key_read','test_gap_10507_function_constructors']
assert not (B/(V+'-runs.json')).exists()
products={str(p):bench.m.sha(p) for arm in arms for p in [B/arm/'perry',B/arm/'libperry_runtime.a',B/arm/'libperry_stdlib.a']}
bench.m.save(V+'-products.json',products)
for case in cases:
    src=root/'test-files'/(case+'.ts')
    oracle=bench.run(V+'-node-'+case,['node','--expose-gc',src],timeout=300)
    assert oracle['rc']==0,oracle
    for arm in arms:
        binary=B/'bin'/f'{V}-{arm}-{case}'; assert not binary.exists()
        env={'PERRY_RUNTIME_DIR':str(B/arm),'PERRY_WORKSPACE_ROOT':str(root),'PERRY_NO_CACHE':'1','PERRY_GC_INSTRUMENTS':'1'}
        r=bench.run(V+'-build-'+arm+'-'+case,[B/arm/'perry','compile',src,'--no-auto-optimize','-o',binary],env,300)
        r.update(case=case,arm=arm,mode='build',source_sha256=bench.m.sha(src)); records.append(r)
        bench.m.save(V+'-runs.json',records); assert r['rc']==0 and r['reason'] is None,r
        for mode in ['default','moving']:
            env={} if mode=='default' else {'PERRY_GC_SCHEDULE_SEED':'7949','PERRY_GC_SCHEDULE_RATE':'1' if case in ['test_gap_gc_container_value_rooting','test_gap_gc_define_properties_key_rooting'] else '0.01','PERRY_GC_SCHEDULE_ALLOC_KB':'4','PERRY_GC_PROTECT_FROMSPACE':'1','PERRY_GC_VERIFY_EVACUATION':'1','PERRY_GC_DIAG':'1','PERRY_GC_TRACE':'1'}
            label=f'{V}-{arm}-{case}-{mode}'
            r=bench.run(label,[binary],env,300)
            r.update(case=case,arm=arm,mode=mode,binary_sha256=bench.m.sha(binary),correct=r['rc']==0 and r['reason'] is None and r['stdout']==oracle['stdout'])
            if mode=='moving':
                lines=(B/'logs'/(label+'.err')).read_text().splitlines()
                cycles=[json.loads(s) for s in lines if s.startswith('{')]
                r.update(copied_objects=sum(c.get('copying_nursery',{}).get('copied_objects',0) for c in cycles),protected=any(s.startswith('[gc-fromspace-protect]') and 'retired_set=' in s for s in lines))
            records.append(r); bench.m.save(V+'-runs.json',records)
            assert r['correct'],r
            if mode=='moving': assert r['copied_objects']>0 and r['protected'],r
            print(arm,case,mode,'correct',r.get('copied_objects'),flush=True)
assert {p:bench.m.sha(p) for p in products}==products
