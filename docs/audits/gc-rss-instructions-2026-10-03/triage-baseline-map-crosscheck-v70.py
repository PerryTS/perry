"""Read-only attribution of the preserved baseline crosscheck abort."""
import pathlib,struct,json,re,bisect,hashlib
B=pathlib.Path('/root/rss-header-20261002'); V='gc-map-prefix-main07-v70-linked'
binary=B/'bin'/f'{V}-once-test_gap_gc_container_value_rooting'
log=B/'logs'/f'{V}-once-test_gap_gc_container_value_rooting-moving.err'
raw=binary.read_bytes(); assert raw[:5]==b'\x7fELF\x02' and raw[5]==1
shoff=struct.unpack_from('<Q',raw,40)[0];entsize,count,names=struct.unpack_from('<HHH',raw,58)
headers=[struct.unpack_from('<IIQQQQIIQQ',raw,shoff+i*entsize) for i in range(count)]
namehead=headers[names];strings=raw[namehead[4]:namehead[4]+namehead[5]]
section=next(h for h in headers if strings[h[0]:].split(b'\0',1)[0]==b'.perry_gcmap')
blob=raw[section[4]:section[4]+section[5]]; origin=section[3]; base=0; functions=[];records=[]
while base+16<=len(blob):
 if blob[base:base+4]!=b'PGCM':
  if not any(blob[base:]):break
  base+=1;continue
 assert blob[base+4]==6
 nf,total=struct.unpack_from('<II',blob,base+8); table=base+16; offsets=table+nf*16; record_index=0
 for i in range(nf):
  relative,stack,n=struct.unpack_from('<iII',blob,table+i*12);address=origin+base+relative
  pcs=list(struct.unpack_from('<'+'I'*n,blob,offsets+record_index*4)) if n else []
  record_index+=n
  if n:
   functions.append(dict(address=address,offsets=pcs,blob=base,index=i))
   records.extend((address+off,address,j) for j,off in enumerate(pcs))
 base=(base+total+7)&~7
records.sort(key=lambda r:r[0]); starts=sorted(set(f['address'] for f in functions));pcs=[r[0] for r in records]
text=log.read_text();failure=re.search(r'for ip (0x[0-9a-f]+)\. lazy=\[\((\d+), \[\], \[\]\)\] eager=\[\]',text); assert failure
actual_ip=int(failure[1],16);actual_fn=int(failure[2]);delta=actual_ip-actual_fn; findings=[]
for f in functions:
 if f['address']%4096 !=actual_fn%4096:continue
 image_base=actual_fn-f['address']; assert image_base%4096==0
 ip=f['address']+delta; owning=starts[bisect.bisect_right(starts,ip)-1]
 if owning!=f['address']:continue
 owned=[r for r in records if r[1]==owning]
 best=min(f['offsets'],key=lambda off:abs(off-delta))
 first=bisect.bisect_left(pcs,owning);last=first
 while last<len(records) and records[last][1]==owning:last+=1
 assumed=records[first:last]
 candidate=min(assumed,key=lambda r:abs(r[0]-ip)) if assumed else None
 if candidate and abs(candidate[0]-ip)<=16:continue
 findings.append(dict(function_image_address=hex(owning),runtime_image_base=hex(image_base),ip_image_address=hex(ip),return_delta=delta,lazy_selected_offset=best,lazy_distance=abs(best-delta),real_owned_records=owned,eager_assumed_contiguous_records=assumed,eager_first_record=records[first] if first<len(records) else None,owner_first_record_pcs=[r[0] for r in owned]))
assert len(findings)==1,findings
proof=dict(binary_sha256=hashlib.sha256(raw).hexdigest(),raw_log_sha256=hashlib.sha256(log.read_bytes()).hexdigest(),runtime_failure_ip=hex(actual_ip),runtime_failure_function=hex(actual_fn),findings=findings,scope='Read-only fixed-offset-table reconstruction of the unchanged control crosscheck abort. No metadata or production code edits.')
(B/'baseline-map-crosscheck-v70-triage.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof,indent=2))
