set pagination off
set breakpoint pending on
set print demangle on
set disable-randomization on
handle SIGPIPE nostop noprint pass
python
import gdb,json
active={}
def tid():return gdb.selected_thread().global_num
class End(gdb.FinishBreakpoint):
 def __init__(self,frame,t):
  self.t=t
  super().__init__(frame,internal=True)
 def stop(self):
  active[self.t]=False
  return False
class Begin(gdb.Breakpoint):
 def stop(self):
  t=tid();active[t]=True
  End(gdb.newest_frame(),t)
  return False
class Alloc(gdb.Breakpoint):
 def __init__(self,symbol,reg):
  self.symbol=symbol;self.reg=reg
  super().__init__(symbol,internal=True)
 def stop(self):
  if not active.get(tid(),False):return False
  size=int(gdb.parse_and_eval('$'+self.reg))
  if size>=65536:
   f=gdb.newest_frame();frames=[]
   for i in range(5):
    if f is None:break
    frames.append(f.name());f=f.older()
   print('ALLOC_REQUEST '+json.dumps(dict(symbol=self.symbol,size=size,frames=frames)))
  return False
Begin('js_zlib_gunzip_sync',internal=True)
for symbol,reg in [('mi_malloc','rdi'),('mi_malloc_aligned','rdi'),('mi_realloc','rsi'),('mi_realloc_aligned','rsi'),('mi_realloc_aligned_at','rsi')]:Alloc(symbol,reg)
end
run
