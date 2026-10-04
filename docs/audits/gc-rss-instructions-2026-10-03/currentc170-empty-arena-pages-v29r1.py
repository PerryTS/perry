"""Read-only pagemap census of block interiors at existing full-GC boundaries.

Uses verified shipping disassembly and independently measured Rust geometry.
Adds no collections and never reads/writes block payloads. Debugger stops and
the existing census perturb RSS; this is diagnosis, not acceptance.
"""
import pathlib,json,subprocess,re,hashlib
B=pathlib.Path('/root/rss-header-20261002');V='gc-currentc170-empty-arena-pages-v29r1'
O=B/V;O.mkdir(exist_ok=False)
geometry=json.loads((B/'primary-arena-geometry-v27/export-r1/geometry-verified.json').read_text())['geometry']
assert geometry['arena']['current']==24 and geometry['block']['size']==48
assert geometry['block']['dead_cycles']==40
assert geometry['block']['data']==16 and geometry['block']['size_field']==24 and geometry['block']['offset']==32
assert geometry['vec']['word_offsets']==dict(pointer=8,capacity=0,length=16)
configs={}
for arm in ['currentc170-base','currentc170-gc']:
    binary=B/'qb6-currentc170-build-v22/bin'/f'gc-qb6-currentc170-v22-noauto-{arm}-tscwork'
    symbols=[s.split() for s in subprocess.check_output(['nm','-n',binary],text=True).splitlines()]
    def symbol(needle):
        rows=[r for r in symbols if len(r)==3 and r[2].endswith(needle)]
        assert len(rows)==1,(needle,rows);return int(rows[0][0],16)
    census=symbol('4walk18arena_space_census')
    asm=subprocess.check_output(['objdump','-d','--no-show-raw-insn',f'--start-address={census}',f'--stop-address={census+1800}',binary],text=True)
    (O/(arm+'.disassembly.txt')).write_text(asm)
    offsets=re.findall(r'lea\s+-0x([0-9a-f]+)\(%rax\),%rax',asm)
    assert len(offsets)>=2 and all(x==offsets[1] for x in offsets[1:]),offsets
    # The first TLS access follows sync_inline_arena_state; capture there.
    assert re.search(rf'{census+0x17:x}:\s+call',asm)
    assert re.search(rf'{census+0x1d:x}:\s+mov\s+%fs:0x0,%rax',asm)
    for pattern in [r'mov\s+0x10\(%rax\),%rcx',r'mov\s+0x8\(%rax\),%rsi',r'shl\s+\$0x4,%rcx',r'add\s+\$0x60,%rsi',r'cmpq\s+\$0x0,0x10\(%rsi\)',r'movdqu\s+0x18\(%rsi\)',r'mov\s+0x118\(%rax,%rbx,8\),%rax']:
        assert re.search(pattern,asm),pattern
    slots={name:symbol('5block'+str(len(name))+name+'4SLOT') for name in ['SURVIVOR_ARENA_0','SURVIVOR_ARENA_1','LONGLIVED_ARENA','OLD_ARENA']}
    configs[arm]=dict(census=census,breakpoint=census+0x1d,arena_tls=-int(offsets[0],16),hot_tls=-int(offsets[1],16),slots=slots)
(O/'shipping-access-proof.json').write_text(json.dumps(configs,indent=2)+'\n')
observer=O/'empty-pages-observer.py'
observer.write_text('''import gdb,json,pathlib,struct,os
CONFIGS=json.loads(pathlib.Path(%r).read_text())
ROOT=pathlib.Path(%r)
def integer(expr):return int(gdb.parse_and_eval(expr))
def read(addr,n):return bytes(gdb.selected_inferior().read_memory(addr,n))
def u64(addr):return struct.unpack('<Q',read(addr,8))[0]
class EmptyPages(gdb.Breakpoint):
 def stop(self):
  config=CONFIGS[ARM];base=integer('$base');fs=integer('$fs_base');hot=fs+config['hot_tls']
  general=fs+config['arena_tls'];assert u64(hot)==general
  arenas=[('nursery_eden',general)]
  for name in ['SURVIVOR_ARENA_0','SURVIVOR_ARENA_1','LONGLIVED_ARENA','OLD_ARENA']:
   index=struct.unpack('<I',read(base+config['slots'][name]+16,4))[0];assert index<768
   ptr=u64(hot+0x118+index*8);assert ptr
   arenas.append(({'SURVIVOR_ARENA_0':'survivor0','SURVIVOR_ARENA_1':'survivor1','LONGLIVED_ARENA':'longlived','OLD_ARENA':'old'}[name],ptr))
  rows=[];page=os.sysconf('SC_PAGE_SIZE');assert page==4096
  with open('/proc/%%d/pagemap'%%gdb.selected_inferior().pid,'rb',buffering=0) as pm:
   for name,arena in arenas:
    cap,ptr,length,current=struct.unpack('<QQQQ',read(arena,32));assert length<=cap<100000
    blocks=[]
    for i in range(length):
     block=ptr+i*48;data,size,offset=struct.unpack('<QQQ',read(block+16,24));assert offset<=size
     start=(data+page-1)//page*page;end=(data+size)//page*page
     entries=[]
     if data and end>start:
      pm.seek(start//page*8);raw=pm.read((end-start)//page*8);assert len(raw)==(end-start)//page*8
      entries=struct.unpack('<'+'Q'*(len(raw)//8),raw)
     resident=sum(bool(e&(1<<63)) for e in entries)*page
     dead_cycles=struct.unpack('<I',read(block+40,4))[0]
     blocks.append(dict(dead_cycles=dead_cycles,index=i,data=data,size=size,offset=offset,current=i==current,interior_bytes=len(entries)*page,resident_interior_bytes=resident,swapped_interior_bytes=sum(bool(e&(1<<62)) for e in entries)*page))
    rows.append(dict(space=name,arena_address=arena,current=current,blocks=blocks,capacity_bytes=sum(x['size'] for x in blocks if x['data']),used_bytes=sum(x['offset'] for x in blocks if x['data'])))
  report=dict(full=integer('$fulls'),spaces=rows)
  with (ROOT/(ARM+'.pages.jsonl')).open('a') as f:f.write(json.dumps(report)+'\\n')
  return False
EmptyPages('*'+repr(CENSUS_SYMBOL)+'+29')
'''%(str(O/'shipping-access-proof.json'),str(O)))
source=(B/'currentc170-natural-arena-timeline-v26r1.py').read_text()
source=source.replace("V = 'gc-currentc170-natural-arena-v26r1'", "V = '"+V+"'")
source=source.replace("[('tscwork', ['1']), ('zodwork', ['200'])]", "[('tscwork', ['1'])]")
source=source.replace("prefix = f'{V}-{arm}-{case}'", "prefix = f'{V}-{arm}-{case}'\n        census_symbol = next(x[2] for x in symbols if len(x)==3 and x[2].endswith('4walk18arena_space_census'))")
source=source.replace("source {observer}\nbreak", "source {observer}\npython ARM={arm!r}; CENSUS_SYMBOL={census_symbol!r}\nsource "+str(observer)+"\nbreak")
exec(compile(source,str(B/'currentc170-natural-arena-timeline-v26r1.py'),'exec'))
