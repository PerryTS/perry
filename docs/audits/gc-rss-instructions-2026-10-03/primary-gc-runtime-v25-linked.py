"""Linked default and genuinely moving GC checks; exact Node output on both arms."""
import json
import bench
B=bench.B; V='gc-private-currentc170-v25-linked'; records=[]
R=B/'primary-gc-runtime-v24'; E=R/'export'; root=R/'source-control'
arms=['control','elf']
assert (E/'independent-build-verification.json').exists()
cases=['test_gap_gc_container_value_rooting','test_gap_gc_define_properties_key_rooting','test_gap_gc_store_ic_old_to_young','test_gap_class_accessor_shape_facts','test_gap_10753_dynamic_key_read','test_gap_10507_function_constructors','test_gap_gc_11789_closure_call_args','test_gap_class_expr_fresh_static_blocks_this','test_gap_byname_function_reads','test_gap_byname_inherited_reads','test_gap_gc_11816_array_callback_element']
assert not (B/(V+'-runs.json')).exists()
products={str(p):bench.m.sha(p) for arm in arms for p in [E/arm/'perry',E/arm/'libperry_runtime.a',E/arm/'libperry_stdlib.a']}
bench.m.save(V+'-products.json',products)
derived=B/'derived-private-v25-linked'; derived.mkdir(exist_ok=False)
original=root/'test-files/test_gap_class_expr_fresh_static_blocks_this.ts'
source=original.read_text()
for statement in ['const B = makeExpr("B");','(globalThis as any).__IDENT = I;','const D = makeDecl("D");']:
    assert source.count(statement)==1
    source=source.replace(statement,statement+'\ngc();')
source='declare function gc(): void;\n'+source
(derived/original.name).write_text(source)
bench.m.save(V+'-derived-source.json',dict(original_source_sha256=bench.m.sha(original),derived_source_sha256=bench.m.sha(derived/original.name),scope='Three explicit collections between fresh class creation and subsequent static reads. Original campaign fixture remains unchanged. Node output must match original.'))
for case in cases:
    src=root/'test-files'/(case+'.ts')
    if case=='test_gap_class_expr_fresh_static_blocks_this':
        src=derived/src.name
    oracle=bench.run(V+'-node-'+case,['node','--expose-gc',src],timeout=300)
    assert oracle['rc']==0,oracle
    if case=='test_gap_class_expr_fresh_static_blocks_this':
        control=bench.run(V+'-node-original-'+case,['node','--expose-gc',original],timeout=300)
        assert control['rc']==0 and control['stdout']==oracle['stdout']
    for arm in arms:
        binary=B/'bin'/f'{V}-{arm}-{case}'; assert not binary.exists()
        env={'PERRY_RUNTIME_DIR':str(E/arm),'PERRY_WORKSPACE_ROOT':str(R/('source-'+arm)),'PERRY_NO_CACHE':'1','PERRY_GC_INSTRUMENTS':'1','CARGO_TARGET_DIR':str(R/('target-linked-v25-'+arm))}
        r=bench.run(V+'-build-'+arm+'-'+case,[E/arm/'perry','compile',src,'--no-auto-optimize','-o',binary],env,300)
        r.update(case=case,arm=arm,mode='build',source_sha256=bench.m.sha(src),compiler_sha256=bench.m.sha(E/arm/'perry'),runtime_sha256=bench.m.sha(E/arm/'libperry_runtime.a'),binary_sha256=bench.m.sha(binary) if r['rc']==0 else None); records.append(r)
        bench.m.save(V+'-runs.json',records); assert r['rc']==0 and r['reason'] is None,r
        for mode in ['default','moving']:
            env={} if mode=='default' else {'PERRY_GC_SCHEDULE_SEED':'7949','PERRY_GC_SCHEDULE_RATE':'1' if case in ['test_gap_gc_container_value_rooting','test_gap_gc_define_properties_key_rooting','test_gap_10753_dynamic_key_read','test_gap_byname_function_reads','test_gap_byname_inherited_reads','test_gap_gc_11816_array_callback_element'] else '0.01','PERRY_GC_SCHEDULE_ALLOC_KB':'0' if case in ['test_gap_10753_dynamic_key_read','test_gap_byname_function_reads','test_gap_byname_inherited_reads','test_gap_gc_11816_array_callback_element'] else '4','PERRY_GC_PROTECT_FROMSPACE':'1','PERRY_GC_VERIFY_EVACUATION':'1','PERRY_GC_DIAG':'1','PERRY_GC_TRACE':'1'}
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

(B/(V+".exit")).write_text("0\n")
