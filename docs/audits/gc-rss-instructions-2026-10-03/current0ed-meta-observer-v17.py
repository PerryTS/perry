"""Read-only Linux/x86-64 current0ed ObjectMeta census at take_census.
Requires exact 16-byte ObjectHeader, 160-byte ObjectMeta payload (168 with GC).
Uses the collector's pass1 marked-vector, never walks guessed addresses.
"""
import collections, gdb, json, os, pathlib, struct
B=pathlib.Path(os.environ['META_ROOT']); prefix=os.environ['META_PREFIX']
FIELDS='prototype attr_key_bits accessor_key_bits flags spill private_evaluation_brand array_subclass_named_prefix_token array_tail_object_hot array_subclass_dense_key array_subclass_dense_slots array_subclass_dense_bounds expando elements descriptor_key_hash descriptor_key_count dictionary_keys proto_serial arguments instance_birth native_state'.split()
SUMMARY={'attr_key_bits','accessor_key_bits','descriptor_key_hash','descriptor_key_count'}
class Observe(gdb.Breakpoint):
 def stop(self):
  inferior=gdb.selected_inferior(); pid=inferior.pid
  cap,ptr,count=struct.unpack('<QQQ',inferior.read_memory(int(gdb.parse_and_eval('$rdx')),24))
  assert 0<count<=cap<=10_000_000,(cap,ptr,count)
  headers=struct.unpack('<'+'Q'*count,inferior.read_memory(ptr,count*8))
  assert list(headers)==sorted(set(headers))
  fd=os.open(f'/proc/{pid}/mem',os.O_RDONLY); pages={}
  def read(addr,size):
   data=b''
   while size:
    page=addr&~4095;off=addr-page;take=min(size,4096-off)
    if page not in pages:pages[page]=os.pread(fd,4096,page)
    assert len(pages[page])==4096
    data+=pages[page][off:off+take];addr+=take;size-=take
   return data
  def words(addr,n):return struct.unpack('<'+'Q'*n,read(addr,n*8))
  live={};counts=collections.Counter();sizes=collections.Counter()
  for h in headers:
   kind,flags,layout,size=struct.unpack('<BBHI',read(h,8));assert flags&5
   if flags&128:continue
   live[h+8]=(kind,size);counts[kind]+=1;sizes[kind]+=size
  owners=collections.Counter();patterns=collections.Counter();summary_only=set();nonzero=collections.Counter();all_patterns=collections.Counter();ordinary_no_meta=0
  for user,(kind,size) in live.items():
   if kind==19:
    assert size==168,(size,user)
    fields=dict(zip(FIELDS,words(user,20))); nz={k for k,v in fields.items() if v}
    all_patterns['+'.join(sorted(nz)) or 'zero']+=1;nonzero.update(nz)
   if kind!=2:continue
   identity,meta=words(user,2)
   if not meta:ordinary_no_meta+=1;continue
   assert live.get(meta)==(19,168),(hex(meta),live.get(meta))
   owners[meta]+=1
   fields=dict(zip(FIELDS,words(meta,20)));nz={k for k,v in fields.items() if v}
   patterns['+'.join(sorted(nz)) or 'zero']+=1
   if nz and nz<=SUMMARY:summary_only.add(meta)
  result=dict(source_base='0ed699b9f26267d5ede9e9aabd12160c842cec81',pass1_headers=count,live_counts=dict(counts),live_bytes=dict(sizes),object_meta_header_inclusive_bytes=168,object_meta_payload_bytes=160,ordinary_objects_without_meta=ordinary_no_meta,ordinary_meta_owners=sum(owners.values()),ordinary_distinct_meta_records=len(owners),ordinary_meta_bytes=len(owners)*168,ordinary_meta_patterns=dict(patterns),all_meta_patterns=dict(all_patterns),all_meta_fields_nonzero=dict(nonzero),ordinary_summary_only_meta_records=len(summary_only),ordinary_summary_only_meta_bytes=len(summary_only)*168,summary_payload_bytes_in_all_meta=counts[19]*32,summary_payload_bytes_in_ordinary_meta=len(owners)*32,ordinary_meta_owner_histogram=dict(collections.Counter(owners.values())))
  result['shapes']=observe_shapes(read)
  result['smaps_rollup']=pathlib.Path(f'/proc/{pid}/smaps_rollup').read_text()
  result['maps']=pathlib.Path(f'/proc/{pid}/maps').read_text()
  os.close(fd)
  (B/f'{prefix}.sharing.json').write_text(json.dumps(result,indent=2)+'\n')
  return False
Observe('*'+repr(os.environ['META_TAKE_SYMBOL']))
