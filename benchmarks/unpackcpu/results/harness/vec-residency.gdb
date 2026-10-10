set pagination off
set breakpoint pending on
set disable-randomization on
handle SIGPIPE nostop noprint pass
python
import gdb,json,struct,os
class Finish(gdb.FinishBreakpoint):
 def __init__(self,frame,slot):
  self.slot=slot
  super().__init__(frame,internal=True)
 def stop(self):
  inf=gdb.selected_inferior();words=struct.unpack('<QQQ',bytes(inf.read_memory(self.slot,24)))
  cap,ptr,length=words
  if 0<length<=cap<1<<31 and ptr>1<<32:
   first=ptr//4096;pages=(ptr%4096+cap+4095)//4096
   with open('/proc/'+str(inf.pid)+'/pagemap','rb') as f:
    f.seek(first*8);data=f.read(pages*8)
   resident=sum(bool(v[0]&(1<<63)) for v in struct.iter_unpack('<Q',data))*4096
   print('VEC_RESIDENCY '+json.dumps(dict(capacity=cap,length=length,resident_bytes=resident,pages=pages)))
  else:print('RESULT_WORDS '+json.dumps(words))
  return False
class Enter(gdb.Breakpoint):
 def stop(self):
  Finish(gdb.newest_frame(),int(gdb.parse_and_eval('$rdi')))
  return False
symbol='perry_ext_zlib::gunzip_bytes' if os.environ['CASE_ARM']=='base' else 'perry_ext_zlib::stream::driver::decode_bytes'
Enter(symbol,internal=True)
end
run
