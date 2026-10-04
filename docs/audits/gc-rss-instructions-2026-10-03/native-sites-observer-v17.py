import gdb,json,pathlib,os,struct
B=pathlib.Path(os.environ['NATIVE_SITES_ROOT'])
prefix=os.environ['NATIVE_SITES_PREFIX']
sites={};hits=0;tracked=0
class Returned(gdb.FinishBreakpoint):
 def __init__(self,frame,size,stack):
  super().__init__(frame,internal=True)
  self.size=size;self.stack=stack
 def stop(self):
  address=int(gdb.parse_and_eval('$rax'))
  if address:sites[str(address)]={'size':self.size,'frames':self.stack}
  return False
class Allocate(gdb.Breakpoint):
 def stop(self):
  global hits,tracked
  hits+=1
  size=int(gdb.parse_and_eval('$rsi'))
  if size>=65536:
   tracked+=1;stack=[];frame=gdb.newest_frame()
   while frame and len(stack)<16:
    stack.append({'pc':frame.pc(),'name':frame.name()})
    frame=frame.older()
   Returned(gdb.newest_frame(),size,stack)
  return False
allocate=Allocate('_mi_malloc_generic',internal=True)
def finish_sites():
 allocate.enabled=False
 (B/f'{prefix}.sites.json').write_text(json.dumps({'generic_hits':hits,'tracked_allocations':tracked,'latest_address_sites':sites},indent=2)+'\n')
def capture_native_pages():
 pid=gdb.selected_inferior().pid
 assert pid>1
 fd=os.open(f'/proc/{pid}/pagemap',os.O_RDONLY)
 flags_fd=os.open('/proc/kpageflags',os.O_RDONLY);page_flags={}
 rows=[]
 for line in pathlib.Path(f'/proc/{pid}/maps').read_text().splitlines():
  parts=line.split(maxsplit=5);name=parts[5] if len(parts)>5 else ''
  if name not in ('','[heap]') and not name.startswith('[anon:'):continue
  start,end=[int(s,16) for s in parts[0].split('-')]
  if parts[1][:3]=='---':continue
  present=[]
  for first in range(start//4096,end//4096,65536):
   n=min(65536,end//4096-first)
   raw=os.pread(fd,n*8,first*8);assert len(raw)==n*8
   for i,(value,) in enumerate(struct.iter_unpack('<Q',raw)):
    if not value>>63:continue
    pfn=value&((1<<55)-1)
    assert pfn,'PFNs restricted; cannot distinguish shared zero pages from RSS'
    if pfn not in page_flags:page_flags[pfn]=struct.unpack('<Q',os.pread(flags_fd,8,pfn*8))[0]
    # KPF_ZERO_PAGE=24: present shared zero pages are not charged to RSS.
    if not page_flags[pfn]&(1<<24):present.append((first+i)*4096)
  rows.append(dict(start=start,end=end,name=name,permissions=parts[1],present_pages=present))
 os.close(fd)
 os.close(flags_fd)
 (B/f'{prefix}.resident-pages.json').write_text(json.dumps(rows)+'\n')
