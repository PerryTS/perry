from pathlib import Path
import mmap,struct,json,hashlib
L=Path('/root/lanes/perry-unpackcpu')
rows=[]
for name in ['hello','upm','fastify','effect','buffer_heavy','worker_heavy']:
 for arm in ['base','fix']:
  p=L/'bins'/arm/name
  with p.open('rb') as f,mmap.mmap(f.fileno(),0,access=mmap.ACCESS_READ) as b:
   h=struct.unpack_from('<16sHHIQQQIHHHHHH',b)
   assert h[0][:6]==b'\x7fELF\x02\x01'
   sec=[struct.unpack_from('<IIQQQQIIQQ',b,h[6]+i*h[11]) for i in range(h[12])]
   names=b[sec[h[13]][4]:sec[h[13]][4]+sec[h[13]][5]]
   named={names[s[0]:].split(b'\0',1)[0].decode():s for s in sec}
   init=named.get('.init_array');addresses=set(struct.unpack_from('<Q',b,o)[0] for o in range(init[4],init[4]+init[5],8)) if init else set()
   syms=named.get('.symtab');strings=sec[syms[6]];strings=b[strings[4]:strings[4]+strings[5]]
   constructors=[];openssl=0;zng=0
   for off in range(syms[4],syms[4]+syms[5],syms[9]):
    sym=struct.unpack_from('<IBBHQQ',b,off)
    if sym[4] in addresses or sym[0] and (strings[sym[0]:sym[0]+4] in [b'EVP_',b'zng_']):
     symbol=strings[sym[0]:].split(b'\0',1)[0].decode(errors='replace')
     if sym[4] in addresses:constructors.append(symbol)
     openssl+=symbol.startswith('EVP_');zng+=symbol.startswith('zng_')
   rows.append(dict(name=name,arm=arm,binary_bytes=len(b),sections={key:named[key][5] for key in ['.text','.rodata','.data.rel.ro','.rela.dyn','.bss','.init_array'] if key in named},constructors=constructors,evp_symbols=openssl,zng_symbols=zng,section_hashes={key:hashlib.sha256(b[named[key][4]:named[key][4]+named[key][5]]).hexdigest() for key in ['.text','.rodata','.init','.data.rel.ro','.rela.dyn'] if key in named}))
(L/'evidence/elf-layout.json').write_text(json.dumps(rows,indent=2)+'\n')
for name in sorted({r['name'] for r in rows}):
 b,f=[r for r in rows if r['name']==name]
 print(name,{k:f['sections'].get(k,0)-b['sections'].get(k,0) for k in set(b['sections'])|set(f['sections'])},'constructors',b['constructors'],f['constructors'])
