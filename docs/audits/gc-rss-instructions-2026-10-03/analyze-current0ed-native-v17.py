"""Current-main native allocation ownership; requested/usable/resident are separate.
Whole resident pages only: mixed-owner pages are never charged to both owners.
"""
import bisect,collections,csv,json,pathlib,re
B=pathlib.Path(__file__).resolve().parent/'qb6-current0ed-build-v14'
M=2**20
def fallback_owner(site):
 patterns=[
  ('try_alloc_block','GC arena backing'),
  ('canonical_keys','Canonical property keys'),
  ('layout::slot_mask','Per-object GC slot-map table'),
  ('layout_note_slot','Per-object GC slot-map table'),
  ('js_register_function_source_static','Function source registry'),
  ('js_register_function_name','Function name registry'),
  ('ObjectHotTables','Object hot tables'),
  ('pic_arena_alloc','Inline cache storage'),
  ('stack_maps::lazy','Stackmap function index'),
  ('gc::malloc::gc_malloc','Malloc-managed GC objects'),
  ('shapes','Shape tables'),
  ('string::intern','String intern storage'),
  ('prop_plan','Property plan caches'),
  ('ExceptionState','Exception state'),
  ('js_gc_register_global_root','Module global roots'),
  ('note_declared_instance_field_name','Declared field names'),
  ('remember_class_keys','Class key cache'),
  ('shape_cache_insert','Class key cache'),
  ('js_register_class_constructor','Class constructor registry'),
  ('MallocState','Malloc GC bookkeeping'),
  ('owner_index_transfer','Property descriptor owner index'),
  ('builtin_closure_metadata','Builtin closure metadata'),
  ('ObjDispatchICEntry','Object dispatch cache'),
  ('page_meta','Arena page tables'),
  ('PropertyAttrs','Property descriptor table'),
  ('parent_dense_store','Class parent table'),
  ('gc::layout_tables','GC layout bookkeeping'),
  ('shadow_stack','Shadow stack')]
 for frame in site['frames'][1:]:
  for text,label in patterns:
   if text in (frame['name'] or ''):return label
 return 'Other matched native tables'

def owner(site):
    names='\n'.join(f['name'] or '' for f in site['frames'])
    compact=re.sub(r'\s+', '', names)
    fallback=fallback_owner(site)
    # Preserve the nearest allocation owner: canonical callers also allocate
    # GC arena backing, which must not be re-labelled as a native key index.
    if fallback=='Canonical property keys':
        if '((usize,u32),u32)' in compact: return 'Canonical keys: by_addr index'
        if '((u32,u64),u32)' in compact: return 'Canonical keys: edges index'
        if 'alloc_node' in names and 'RawVec' in names and 'RawTable' not in names:
            return 'Canonical keys: node vector'
        return 'Canonical keys: unresolved allocation'
    return fallback

def analyze(run):
    case=run['case']; rep=run['repeat']; prefix=f'current0ed-native-v17-{case}-{rep}'
    assert run['correct'] and run['stdout'].count('HEAP_DUMP_RESULT=0')==1
    sites=json.loads((B/f'{prefix}.sites.json').read_text())
    rows=list(csv.reader((B/f'{prefix}.csv').open()))
    blocks=sorted((int(r[1]),int(r[2])) for r in rows if r[0]=='B')
    areas=[r for r in rows if r[0]=='A']
    assert all(a+n<=b for (a,n),(b,_) in zip(blocks,blocks[1:]))
    starts=[a for a,_ in blocks]
    shapes=json.loads((B/f'{prefix}.shape-storage.json').read_text())
    shape_blocks={}; shape_used=collections.Counter()
    for addr,size,label in shapes['spans']:
        if not size: continue
        i=bisect.bisect_right(starts,addr)-1
        assert i>=0 and addr+size<=blocks[i][0]+blocks[i][1],(label,addr,size)
        base=blocks[i][0]
        assert base not in shape_blocks,('two distinct shape owners in one allocator block',base)
        shape_blocks[base]='Shapes: '+label
        shape_used['Shapes: '+label]+=size
    allocated=collections.Counter(); counts=collections.Counter(); samples={}; intervals=[]
    for addr,size in blocks:
        site=sites['latest_address_sites'].get(str(addr))
        group=shape_blocks.get(addr,'Unattributed small/fast-path allocations')
        if addr not in shape_blocks and site and size//2<site['size']<=size:
            group=owner(site)
            if len(samples.setdefault(group,[]))<3: samples[group].append(site)
        allocated[group]+=size; counts[group]+=1
        if group=='GC arena backing':
            assert site['size']==M+15 and size>=M and addr%16==0,(addr,size,site)
            intervals.extend([(addr,addr+M,group),(addr+M,addr+size,'Allocator rounding after arena payload')])
        else: intervals.append((addr,addr+size,group))
    # Adjacent 2KiB shape chunks can together cover a whole page. Merge only
    # when no gap and exactly the same owner; never invent ownership of gaps.
    merged=[]
    for start,end,group in intervals:
        if start==end: continue
        if merged and merged[-1][1]==start and merged[-1][2]==group:
            merged[-1]=(merged[-1][0],end,group)
        else: merged.append((start,end,group))
    starts=[r[0] for r in merged]; resident=collections.Counter(); page_names={}
    for mapping in json.loads((B/f'{prefix}.resident-pages.json').read_text()):
        for page in mapping['present_pages']:
            assert page not in page_names
            page_names[page]=mapping['name']
    for page,name in page_names.items():
        i=bisect.bisect_right(starts,page)-1
        if i>=0 and merged[i][0]<=page and page+4096<=merged[i][1]: group=merged[i][2]
        elif (i>=0 and merged[i][1]>page) or (i+1<len(merged) and merged[i+1][0]<page+4096):
            group='Mixed allocation/free-slot pages'
        elif name=='[anon:mimalloc]': group='Allocator pages outside visited allocations'
        else: group='Other anonymous mappings'
        resident[group]+=4096
    smaps_anon=0; is_anon=False
    for line in (B/'logs'/f'{prefix}.before.smaps').read_text().splitlines():
        if re.match(r'^[0-9a-f]+-[0-9a-f]+ ',line):
            fields=line.split(maxsplit=5); name=fields[5] if len(fields)>5 else ''
            is_anon=name in ('','[heap]') or name.startswith('[anon:')
        elif line.startswith('Rss:') and is_anon: smaps_anon+=int(line.split()[1])*1024
    assert smaps_anon==len(page_names)*4096==sum(resident.values()),(smaps_anon,len(page_names)*4096)
    return dict(case=case,repeat=rep,selected_full=run['selected_full'],observation_kind=run['observation_kind'],
        binary_sha256=run['binary_sha256'],allocator_blocks=len(blocks),
        allocated_usable_bytes=dict(allocated),allocation_counts=dict(counts),
        shape_used_bytes=dict(shape_used),shape_observed=shapes,
        resident_anonymous_bytes=smaps_anon,resident_whole_pages_by_owner_bytes=dict(resident),
        area_committed_bytes=sum(int(r[4]) for r in areas),
        area_reserved_bytes=sum(int(r[3]) for r in areas),sample_stacks=samples,
        limitation='Native sites cover generic allocations >=64KiB only; exact shape spans supplement small blocks. Mixed pages remain unassigned.')

if __name__=='__main__':
    runs=json.loads((B/'current0ed-native-v17-runs.json').read_text())
    assert {(r['case'],r['repeat']) for r in runs}=={(c,r) for c in ['tscwork','zodwork'] for r in range(2)}
    result=[analyze(r) for r in runs]
    (B/'CURRENT0ED-NATIVE-ATTRIBUTION-v17.json').write_text(json.dumps(result,indent=2)+'\n')
    for r in result:
        print(r['case'],r['repeat'],'anonymous RSS MiB',r['resident_anonymous_bytes']/M)
        print('usable MiB',{k:round(v/M,3) for k,v in sorted(r['allocated_usable_bytes'].items(),key=lambda kv:-kv[1])})
