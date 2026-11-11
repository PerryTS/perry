#!/usr/bin/env python3
"""Native input values, exact per-thread instruction positions and module returns.

Only instrumented executables exposing js_snap_entry_body get return probes.
A guarded-evaluation return is recorded after its registry commit, not at
the earlier census callback exit. Runtime perf reads calibrate the two
counter domains exactly; account.py checks every census Ir charge.
Unknown syscalls and unreadable buffers remain explicit, never silently
certified. Crypto entry probes audit otherwise unhooked RNG providers;
their names are evidence, never an image-admission rule.
This diagnostic companion is never linked into a produced executable.
"""
import ctypes as C, errno, json, os, signal, struct, sys, subprocess, hashlib
from pathlib import Path
L=C.CDLL(None,use_errno=True)
L.ptrace.restype=C.c_long
L.syscall.restype=C.c_long
TRACEME=0; SYSCALL=24; SETOPTIONS=0x4200; GETEVENTMSG=0x4201; INFO=0x420e
WALL=0x40000000
NAMES={0:"read",1:"write",2:"open",3:"close",4:"stat",5:"fstat",6:"lstat",
7:"poll",8:"lseek",9:"mmap",10:"mprotect",11:"munmap",12:"brk",13:"rt_sigaction",
14:"rt_sigprocmask",15:"rt_sigreturn",16:"ioctl",17:"pread64",18:"pwrite64",
19:"readv",20:"writev",21:"access",22:"pipe",23:"select",24:"sched_yield",
25:"mremap",28:"madvise",32:"dup",33:"dup2",35:"nanosleep",39:"getpid",
41:"socket",42:"connect",43:"accept",44:"sendto",45:"recvfrom",46:"sendmsg",
47:"recvmsg",48:"shutdown",49:"bind",50:"listen",51:"getsockname",52:"getpeername",
53:"socketpair",54:"setsockopt",55:"getsockopt",56:"clone",57:"fork",58:"vfork",
59:"execve",60:"exit",61:"wait4",62:"kill",63:"uname",72:"fcntl",73:"flock",
74:"fsync",75:"fdatasync",78:"getdents",79:"getcwd",80:"chdir",81:"fchdir",
82:"rename",83:"mkdir",84:"rmdir",85:"creat",87:"unlink",89:"readlink",
95:"umask",96:"gettimeofday",97:"getrlimit",98:"getrusage",99:"sysinfo",
102:"getuid",104:"getgid",105:"setuid",106:"setgid",107:"geteuid",108:"getegid",
109:"setpgid",110:"getppid",111:"getpgrp",112:"setsid",115:"getgroups",116:"setgroups",
131:"sigaltstack",157:"prctl",135:"personality",137:"statfs",138:"fstatfs",158:"arch_prctl",186:"gettid",
202:"futex",204:"sched_getaffinity",217:"getdents64",218:"set_tid_address",
228:"clock_gettime",229:"clock_getres",230:"clock_nanosleep",231:"exit_group",
232:"epoll_wait",233:"epoll_ctl",234:"tgkill",257:"openat",262:"newfstatat",
267:"readlinkat",270:"pselect6",271:"ppoll",273:"set_robust_list",281:"epoll_pwait",
288:"accept4",290:"eventfd2",441:"epoll_pwait2",291:"epoll_create1",292:"dup3",293:"pipe2",298:"perf_event_open",299:"recvmmsg",
302:"prlimit64",307:"sendmmsg",318:"getrandom",332:"statx",334:"rseq",435:"clone3",
436:"close_range",439:"faccessat2"}
REP={3,8,9,10,11,12,13,14,15,24,25,28,32,33,60,72,73,158,186,202,218,231,233,234,273,291,292,293,298,334,436}
def ptrace(req,pid,addr=0,data=0):
 C.set_errno(0)
 r=L.ptrace(req,pid,C.c_void_p(addr),C.c_void_p(data))
 if r==-1 and C.get_errno():
  e=C.get_errno()
  raise OSError(e,os.strerror(e))
 return r
def counter(pid):
 a=(C.c_uint64*8)()
 a[0]=64<<32; a[1]=1; a[5]=(1<<5)|(1<<6)
 fd=L.syscall(298,C.byref(a),pid,-1,-1,0)
 if fd<0: raise OSError(C.get_errno(),"perf_event_open")
 return fd
def ir(fd):
 return struct.unpack("Q",os.read(fd,8))[0]
def signed(x):
 return C.c_int64(x).value
def main():
 output=sys.argv[1]; command=sys.argv[2:]
 if command and command[0]=="--":command=command[1:]
 pid=os.fork()
 if pid==0:
  ptrace(TRACEME,0)
  os.kill(os.getpid(),signal.SIGSTOP)
  os.execvpe(command[0],command,os.environ)
 _,status=os.waitpid(pid,0)
 ptrace(SETOPTIONS,pid,0,1|(1<<1)|(1<<2)|(1<<3)|(1<<4)|(1<<20))
 active={pid:{"perf":counter(pid),"entry":None,"mem":None,"diagnostic_fds":set(),"logger_depth":0,"funnel":0}}
 sequence=0; main_rc=None; failures=[]
 out=open(output,"w",buffering=1)
 out.write(json.dumps({"kind":"collector","sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),"command":command})+"\n")
 def memory(tid,addr,n):
  if n==0:return b""
  if not addr:return None
  state=active[tid]
  if state["mem"] is None:
   try:state["mem"]=os.open(f"/proc/{tid}/mem",os.O_RDONLY)
   except OSError:return None
  try:
   data=os.pread(state["mem"],n,addr)
   return data if len(data)==n else None
  except (OSError,OverflowError):return None
 def string(tid,addr):
  if not addr:return None
  data=bytearray()
  while True:
   # Do not cross a page when reading a potentially last-page C string.
   n=min(256,4096-((addr+len(data))&4095))
   chunk=memory(tid,addr+len(data),n)
   if chunk is None:return {"unreadable":addr+len(data)}
   end=chunk.find(b"\0")
   if end>=0:
    data.extend(chunk[:end]);return data.hex()
   data.extend(chunk)
 def blob(tid,addr,n):
  data=memory(tid,addr,n)
  if data is None:
   failures.append({"tid":tid,"address":addr,"length":n})
   return {"unreadable":addr,"length":n}
  return data.hex()
 def iov(tid,addr,count,returned):
  raw=memory(tid,addr,count*16)
  if raw is None:return {"unreadable_iov":addr}
  values=[]
  for i in range(count):
   base,length=struct.unpack_from("QQ",raw,i*16)
   size=min(length,max(0,returned))
   values.append(blob(tid,base,size));returned-=size
  return values
 breakpoints={}; returns={}; audit_names={}; probe_number=0
 def message(tid,address,returned):
  header=memory(tid,address,56)
  if header is None:return {"unreadable_msghdr":address}
  name,nlen,vec,count,control,clen,flags=struct.unpack("QI4xQQQQi4x",header)
  return {"name":blob(tid,name,nlen) if name and nlen else "",
   "iov":iov(tid,vec,count,returned),"control":blob(tid,control,clen) if control and clen else "",
   "flags":flags}
 exe_text={}
 def scan_pcs(tid,window):
  if tid not in exe_text:
   exe=os.readlink(f"/proc/{tid}/exe");spans=[]
   for line in open(f"/proc/{tid}/maps"):
    f=line.split(None,5)
    if len(f)==6 and f[5].strip()==exe and "x" in f[1]:
     lo,hi=(int(x,16) for x in f[0].split("-"));spans.append((lo,hi))
   exe_text[tid]=spans
  out=[]
  for i in range(0,len(window)-7,8):
   w=struct.unpack_from("Q",window,i)[0]
   if not any(lo+7<=w<hi for lo,hi in exe_text[tid]):continue
   b=memory(tid,w-7,7)
   if b is None:continue
   if b[2]==0xe8 or (b[5]==0xff and (b[6]>>3)&7==2) or (b[4]==0xff and (b[5]>>3)&7==2) or (b[1]==0xff and (b[2]>>3)&7==2) or (b[0]==0xff and (b[1]>>3)&7==2):
    out.append(w)
    if len(out)>=12:break
  return out
 def put_breakpoint(tid,address,kind):
  if address in breakpoints:return
  word=ptrace(1,tid,address)&((1<<64)-1)
  ptrace(4,tid,address,(word&~255)|0xcc)
  breakpoints[address]=(word,kind)
 def install_probes(tid):
  # Probe a sealed runtime ABI, not a module-name inventory.
  exe=os.readlink(f"/proc/{tid}/exe")
  symbols={}; audit={}
  result=subprocess.run(["nm","-n","--defined-only",exe],capture_output=True,text=True)
  if result.returncode:return
  for line in result.stdout.splitlines():
   fields=line.split()
   if len(fields)==3:
    if fields[2].startswith(("js_crypto_","js_webcrypto_","js_vm_")) or fields[2] in ("js_eval_in_scope","js_function_constructor","js_function_new"):
     audit.setdefault(int(fields[0],16),[]).append(fields[2])
    if fields[2] in ("js_snap_entry_body","js_run_module_init_guarded"):
     symbols[fields[2]]=int(fields[0],16)
  if not all(k in symbols for k in ("js_snap_entry_body","js_run_module_init_guarded")):return
  with open(exe,"rb") as f:header=f.read(20)
  bias=0
  if struct.unpack_from("<H",header,16)[0]==3:
   for line in open(f"/proc/{tid}/maps"):
    fields=line.split(None,5)
    if len(fields)==6 and fields[5].strip()==exe and int(fields[2],16)==0:
     bias=int(fields[0].split("-")[0],16);break
  address=symbols["js_run_module_init_guarded"]+bias
  put_breakpoint(tid,address,"entry")
  for address0,names in audit.items():
   pointer=address0+bias
   audit_names[pointer]=names
   put_breakpoint(tid,pointer,"audit")
  out.write(json.dumps({"kind":"probes_installed","tid":tid,"guard_entry":address,"ir":ir(active[tid]["perf"])})+"\n")
 def handle_probe(tid):
  nonlocal probe_number
  regs=(C.c_ulonglong*27)()
  ptrace(12,tid,0,C.addressof(regs))
  address=int(regs[16])-1
  if address not in breakpoints:return False
  info=C.create_string_buffer(128)
  ptrace(0x4202,tid,0,C.addressof(info))
  code=struct.unpack_from("i",info.raw,8)[0]
  if code not in (1,128):return False
  original,kind=breakpoints[address]
  pos=ir(active[tid]["perf"])
  if kind=="entry":
   raw=memory(tid,int(regs[19]),8)
   if raw is None:raise RuntimeError("guard return address unreadable")
   return_address=struct.unpack("Q",raw)[0]
   probe_number+=1
   record={"kind":"guard_enter","guard_id":probe_number,"tid":tid,"init_addr":int(regs[14]),
      "guard_addr":int(regs[13]),"return_pc":return_address,"ir":pos,"pc":address}
   returns.setdefault(return_address,[]).append(record)
   put_breakpoint(tid,return_address,"return")
   out.write(json.dumps(record,separators=(",",":"))+"\n")
  elif kind=="audit":
   fp=C.create_string_buffer(512)
   ptrace(14,tid,0,C.addressof(fp))
   out.write(json.dumps({"kind":"native_api_enter","tid":tid,"ir":pos,"pc":address,
      "api":audit_names[address],"gpr":[int(regs[i]) for i in (14,13,12,11,9,8)],
      "xmm":fp.raw[160:288].hex()},separators=(",",":"))+"\n")
  else:
   pending=returns.get(address)
   if not pending:raise RuntimeError("guard completion without entry")
   previous=pending.pop()
   record=dict(previous,kind="guard_complete",ir=pos,pc=address)
   out.write(json.dumps(record,separators=(",",":"))+"\n")
  # Execute exactly the replaced instruction, then restore the entry probe.
  ptrace(4,tid,address,original)
  regs[16]=address
  ptrace(13,tid,0,C.addressof(regs))
  ptrace(9,tid)
  got,status=os.waitpid(tid,WALL)
  if got!=tid or not os.WIFSTOPPED(status) or os.WSTOPSIG(status)!=signal.SIGTRAP:
   raise RuntimeError(f"probe single-step did not stop normally: {got} {status}")
  if kind in ("entry","audit") or returns.get(address):
   ptrace(4,tid,address,(original&~255)|0xcc)
  else:
   del breakpoints[address]
  return True
 ptrace(SYSCALL,pid)
 while active:
  tid,status=os.waitpid(-1,WALL)
  if os.WIFEXITED(status) or os.WIFSIGNALED(status):
   rc=os.WEXITSTATUS(status) if os.WIFEXITED(status) else 128+os.WTERMSIG(status)
   state=active.pop(tid,None)
   if state:
    os.close(state["perf"])
    if state["mem"] is not None:os.close(state["mem"])
   if tid==pid:main_rc=rc
   out.write(json.dumps({"kind":"exit","tid":tid,"rc":rc})+"\n");continue
  if tid not in active:
   active[tid]={"perf":counter(tid),"entry":None,"mem":None,"diagnostic_fds":set(),"logger_depth":0,"funnel":0}
  sig=os.WSTOPSIG(status); event=status>>16
  if event in (1,2,3):
   child=C.c_ulonglong()
   ptrace(GETEVENTMSG,tid,0,C.addressof(child))
   out.write(json.dumps({"kind":"new_task","parent":tid,"tid":child.value,"ir":ir(active[tid]["perf"]),"policy":"STOP"})+"\n")
  elif event==4:
   state=active[tid]
   if state["mem"] is not None:os.close(state["mem"])
   state["mem"]=None
   breakpoints.clear();returns.clear();audit_names.clear()
   install_probes(tid)
  elif sig==(signal.SIGTRAP|0x80):
   buf=C.create_string_buffer(128)
   ptrace(INFO,tid,128,C.addressof(buf))
   raw=buf.raw; op=raw[0]; ip=struct.unpack_from("Q",raw,8)[0]
   state=active[tid]; pos=ir(state["perf"])
   if op==1:
    nr,*args=struct.unpack_from("7Q",raw,24)
    sequence+=1
    entry={"kind":"syscall","seq":sequence,"tid":tid,"ir_enter":pos,"ip":ip,"nr":nr,"name":NAMES.get(nr,"UNKNOWN"),"args":args,"policy":"representation" if nr in REP else "STOP"}
    a=args
    # Optional raw stack evidence for source audit. These words are not a
    # backtrace and never establish an input's eligibility or pointer shape.
    if state["logger_depth"]:
     entry["policy"]="diagnostic"
    if nr not in REP and not state["logger_depth"] and not(nr in (0,1) and a[0] in state["diagnostic_fds"]):
     regs=(C.c_ulonglong*27)()
     ptrace(12,tid,0,C.addressof(regs))
     sp=int(regs[19]);entry["rsp"]=sp
     window=memory(tid,sp,min(2048,4096-(sp&4095)))
     entry["stack_window"]=window.hex() if window is not None else None
     # SNAP25 C1: call-preceded return addresses inside the executable's
     # text, innermost first (a stack scan, labelled as such).
     if window is not None:entry["pcs_scan"]=scan_pcs(tid,window)
    if nr in (2,4,6,21,80,82,83,84,85,87,89,137):entry["path"]=string(tid,a[0])
    elif nr in (257,262,267,439):entry["path"]=string(tid,a[1])
    if nr==59:
     entry["path"]=string(tid,a[0]); argv=[]; index=0
     while True:
      item=memory(tid,a[1]+8*index,8)
      if item is None:entry["unsupported_payload"]=True;break
      ptr=struct.unpack("Q",item)[0]
      if not ptr:break
      argv.append(string(tid,ptr));index+=1
     entry["argv"]=argv
     envp=[];index=0
     while True:
      item=memory(tid,a[2]+8*index,8)
      if item is None:entry["unsupported_payload"]=True;break
      pointer=struct.unpack("Q",item)[0]
      if not pointer:break
      envp.append(string(tid,pointer));index+=1
     entry["environment"]=envp
    if nr==157 and a[0]==0x53564d41:entry["vma_name"]=string(tid,a[4])
    if nr==157 and a[0]==15:entry["input_name"]=string(tid,a[1])
    if nr in (1,18,44):entry["input_bytes"]=blob(tid,a[1],a[2])
    elif nr==20:entry["input_bytes"]=iov(tid,a[1],a[2],1<<63)
    elif nr==46:entry["message"]=message(tid,a[1],1<<63)
    elif nr in (42,49):entry["sockaddr"]=blob(tid,a[1],a[2])
    elif nr==54:entry["option"]=blob(tid,a[3],a[4])
    if nr==8 and (a[0]&0xffffffff)==0xffffffff and a[2]==0x5322:
     entry["kind"]="logger_scope";entry["policy"]="diagnostic"
     state["logger_depth"] += 1 if a[1] else -1
     if state["logger_depth"]<0:raise RuntimeError("unbalanced logger scope")
    if nr==8 and (a[0]&0xffffffff)==0xffffffff and a[2]==0x5323:
     # SNAP25 C2: the runtime funnel now open on this thread.
     entry["kind"]="funnel_marker";entry["policy"]="diagnostic";state["funnel"]=a[1]
    elif state["funnel"]:entry["funnel"]=state["funnel"]
    if nr==8 and (a[0]&0xffffffff)==0xffffffff and a[2]==0x5321:
     entry["kind"]="runtime_marker";entry["runtime_seq"]=a[1]
    if nr in (0,1,8) and a[0] in state["diagnostic_fds"]:entry["policy"]="diagnostic"
    state["entry"]=entry
   elif op==2 and state["entry"] is not None:
    e=state["entry"];state["entry"]=None
    ret=struct.unpack_from("q",raw,24)[0]
    e["ir_exit"]=pos;e["ret"]=ret;e["is_error"]=bool(raw[32])
    a=e["args"];nr=e["nr"]
    if ret>=0:
     if nr==298 or (nr in (2,257) and e["policy"]=="diagnostic"):state["diagnostic_fds"].add(ret)
     if nr==9 and C.c_int(a[4]).value>=0:
      # A file-backed mapping is an input/resource boundary even if its
      # contents are first used later. Capture this observation; subsequent
      # concurrently changed mapped bytes cannot be certified by a syscall
      # tracer and therefore the mapping is a STOP.
      try:
       mapped_fd=os.open(f"/proc/{tid}/fd/{a[4]}",os.O_RDONLY)
       try:
        st=os.fstat(mapped_fd);length=min(a[1],max(0,st.st_size-a[5]))
        data=os.pread(mapped_fd,length,a[5])
        e["file_mapping"]={"file_size":st.st_size,"offset":a[5],"length":a[1],
          "observed_file_bytes":data.hex(),"complete":len(data)==length,
          "note":"file observation at mmap; concurrent later mapped changes require memory instrumentation"}
        if len(data)!=length:e["complete"]=False
       finally:os.close(mapped_fd)
      except OSError as error:
       e["file_mapping"]={"error":error.errno};e["complete"]=False
      e["policy"]="STOP"
     if nr in (0,17,45,78,217):e["bytes"]=blob(tid,a[1],ret)
     elif nr==19:e["bytes"]=iov(tid,a[1],a[2],ret)
     elif nr==318:e["bytes"]=blob(tid,a[0],ret)
     elif nr in (79,89):e["bytes"]=blob(tid,a[0] if nr==79 else a[1],ret)
     elif nr==267:e["bytes"]=blob(tid,a[2],ret)
     elif nr in (4,5,6):e["struct"]=blob(tid,a[1],144)
     elif nr==262:e["struct"]=blob(tid,a[2],144)
     elif nr==332:e["struct"]=blob(tid,a[4],256)
     elif nr in (228,229):e["struct"]=blob(tid,a[1],16)
     elif nr==96:
      e["struct"]=blob(tid,a[0],16) if a[0] else None
      e["timezone"]=blob(tid,a[1],8) if a[1] else None
     elif nr==63:e["struct"]=blob(tid,a[0],390)
     elif nr==99:e["struct"]=blob(tid,a[0],112)
     elif nr in (97,302):e["struct"]=blob(tid,a[1] if nr==97 else a[3],16) if (nr==97 or a[3]) else None
     elif nr==98:e["struct"]=blob(tid,a[1],144)
     elif nr==204:e["struct"]=blob(tid,a[2],ret)
     elif nr==115:e["struct"]=blob(tid,a[1],ret*4) if a[1] else None
     elif nr in (13,14):
      e["old_signal_state"]=blob(tid,a[2],32 if nr==13 else a[3]) if a[2] else None
     elif nr==131:e["old_signal_stack"]=blob(tid,a[1],24) if a[1] else None
     elif nr in (137,138):e["struct"]=blob(tid,a[1],120)
     elif nr==158:
      if a[0] in (0x1003,0x1004):e["tls_pointer"]=blob(tid,a[1],8)
     elif nr in (23,270):
      length=((a[0]+63)//64)*8
      e["fdsets"]=[blob(tid,address,length) if address else None for address in a[1:4]]
      e["timeout"]=blob(tid,a[4],16) if a[4] else None
     elif nr==61:
      e["wait_status"]=blob(tid,a[1],4) if a[1] else None
      e["usage"]=blob(tid,a[3],144) if a[3] else None
     elif nr==157:
      if a[0]==16:e["thread_name"]=blob(tid,a[1],16)
      elif a[0]==40:e["struct"]=blob(tid,a[1],8)
      elif a[0] in (2,5,9,11,19,25,37):e["struct"]=blob(tid,a[1],4)
      elif a[0] not in (1,3,4,6,7,8,10,12,13,14,15,17,18,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36,38,39,41,42,43,44,45,46,47,50,51,52,53,54,55,56,57,58,59,63,64,65,66,67,68,0x53564d41,0x59616d61):
       e["unsupported_payload"]=True
     elif nr==16:
      if a[1] in (0x5413,0x5401,0x541b):e["struct"]=blob(tid,a[2],{0x5413:8,0x5401:36,0x541b:4}[a[1]])
      else:e["unsupported_payload"]=True
     elif nr in (7,271):
      e["struct"]=blob(tid,a[0],a[1]*8)
      if nr==271 and a[2]:e["timeout"]=blob(tid,a[2],16)
     elif nr in (232,281,441):e["struct"]=blob(tid,a[1],ret*12)
     elif nr in (43,51,52,288) and a[1] and a[2]:
      size=memory(tid,a[2],4)
      e["sockaddr"]=blob(tid,a[1],struct.unpack("I",size)[0]) if size else {"unreadable_socklen":a[2]}
     elif nr==55:
      size=memory(tid,a[4],4)
      e["option"]=blob(tid,a[3],struct.unpack("I",size)[0]) if size else {"unreadable_socklen":a[4]}
     elif nr==53:e["fds"]=blob(tid,a[3],8)
     elif nr in (22,293):e["fds"]=blob(tid,a[0],8)
     elif nr==47:e["message"]=message(tid,a[1],ret)
     elif nr in (299,307):
      messages=[]
      for index in range(ret):
       item=a[1]+index*64
       length=memory(tid,item+56,4)
       if length is None:messages.append({"unreadable_mmsghdr":item});continue
       messages.append(message(tid,item,struct.unpack("I",length)[0]))
      e["messages"]=messages
    if ret==-errno.EINTR and nr in (35,230):
     address=a[1] if nr==35 else a[3]
     e["remaining_time"]=blob(tid,address,16) if address else None
    def unreadable(value):
     if isinstance(value,dict):return any(str(k).startswith("unreadable") or unreadable(v) for k,v in value.items())
     if isinstance(value,list):return any(unreadable(v) for v in value)
     return False
    if e["name"]=="UNKNOWN" or e.get("unsupported_payload") or e.get("complete") is False or unreadable(e):e["complete"]=False
    else:e["complete"]=True
    out.write(json.dumps(e,separators=(",",":"))+"\n")
  elif event == 0 and sig==signal.SIGTRAP and handle_probe(tid):
   pass
  elif event == 0 and sig != signal.SIGSTOP:
   information=C.create_string_buffer(128)
   ptrace(0x4202,tid,0,C.addressof(information))
   out.write(json.dumps({"kind":"signal","tid":tid,"signal":sig,"ir":ir(active[tid]["perf"]),
     "siginfo_hex":information.raw.hex()})+"\n")
   ptrace(SYSCALL,tid,0,sig);continue
  ptrace(SYSCALL,tid)
 out.write(json.dumps({"kind":"summary","main_rc":main_rc,"memory_failures":failures})+"\n")
 out.close()
 return main_rc if main_rc is not None else 1
if __name__=="__main__":
 sys.exit(main())

