#!/usr/bin/env python3
"""Lossless syscall evidence plus a per-thread user instruction counter.
Unknown syscalls stay explicit and invalidate input-completeness claims.
This is a diagnostic companion, never part of a produced executable.
"""
import ctypes as C, errno, json, os, signal, struct, sys
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
 r=L.ptrace(req,pid,C.c_void_p(addr),C.c_void_p(data))
 if r==-1:
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
 active={pid:{"perf":counter(pid),"entry":None,"mem":None,"diagnostic_fds":set()}}
 sequence=0; main_rc=None; failures=[]
 out=open(output,"w")
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
   active[tid]={"perf":counter(tid),"entry":None,"mem":None,"diagnostic_fds":set()}
  sig=os.WSTOPSIG(status); event=status>>16
  if event in (1,2,3):
   child=C.c_ulonglong()
   ptrace(GETEVENTMSG,tid,0,C.addressof(child))
   out.write(json.dumps({"kind":"new_task","parent":tid,"tid":child.value,"ir":ir(active[tid]["perf"]),"policy":"STOP"})+"\n")
  elif event==4:
   state=active[tid]
   if state["mem"] is not None:os.close(state["mem"])
   state["mem"]=None
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
    if nr in (2,4,6,21,80,82,83,84,85,87,89,137):entry["path"]=string(tid,a[0])
    elif nr in (257,262,267,439):entry["path"]=string(tid,a[1])
    if nr==59:
     entry["path"]=string(tid,a[0]); argv=[]; index=0
     while True:
      item=memory(tid,a[1]+8*index,8)
      if item is None:break
      ptr=struct.unpack("Q",item)[0]
      if not ptr:break
      argv.append(string(tid,ptr));index+=1
     entry["argv"]=argv
    if nr==157 and a[0]==0x53564d41:entry["vma_name"]=string(tid,a[4])
    if nr==157 and a[0]==15:entry["input_name"]=string(tid,a[1])
    if nr in (1,18,44):entry["input_bytes"]=blob(tid,a[1],a[2])
    elif nr==20:entry["input_bytes"]=iov(tid,a[1],a[2],1<<63)
    elif nr in (42,49):entry["sockaddr"]=blob(tid,a[1],a[2])
    elif nr==54:entry["option"]=blob(tid,a[3],a[4])
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
     if nr==298:state["diagnostic_fds"].add(ret)
     if nr in (0,17,45,78,217):e["bytes"]=blob(tid,a[1],ret)
     elif nr==19:e["bytes"]=iov(tid,a[1],a[2],ret)
     elif nr==318:e["bytes"]=blob(tid,a[0],ret)
     elif nr in (79,89):e["bytes"]=blob(tid,a[0] if nr==79 else a[1],ret)
     elif nr==267:e["bytes"]=blob(tid,a[2],ret)
     elif nr in (4,5,6):e["struct"]=blob(tid,a[1],144)
     elif nr==262:e["struct"]=blob(tid,a[2],144)
     elif nr==332:e["struct"]=blob(tid,a[4],256)
     elif nr in (228,229):e["struct"]=blob(tid,a[1],16)
     elif nr==96:e["struct"]=blob(tid,a[0],16)
     elif nr==63:e["struct"]=blob(tid,a[0],390)
     elif nr==99:e["struct"]=blob(tid,a[0],112)
     elif nr in (97,302):e["struct"]=blob(tid,a[1] if nr==97 else a[3],16) if (nr==97 or a[3]) else None
     elif nr==98:e["struct"]=blob(tid,a[1],144)
     elif nr==204:e["struct"]=blob(tid,a[2],ret)
     elif nr==115:e["struct"]=blob(tid,a[1],ret*4) if a[1] else None
     elif nr==157:
      if a[0]==16:e["thread_name"]=blob(tid,a[1],16)
      elif a[0]==40:e["struct"]=blob(tid,a[1],8)
      elif a[0] in (2,37):e["struct"]=blob(tid,a[1],4)
     elif nr==16:
      if a[1] in (0x5413,0x5401,0x541b):e["struct"]=blob(tid,a[2],{0x5413:8,0x5401:36,0x541b:4}[a[1]])
      else:e["unsupported_payload"]=True
     elif nr in (7,271):e["struct"]=blob(tid,a[0],a[1]*8)
     elif nr in (232,281,441):e["struct"]=blob(tid,a[1],ret*12)
     elif nr in (43,51,52,288) and a[1] and a[2]:
      size=memory(tid,a[2],4)
      e["sockaddr"]=blob(tid,a[1],struct.unpack("I",size)[0]) if size else {"unreadable_socklen":a[2]}
     elif nr==55:
      size=memory(tid,a[4],4)
      e["option"]=blob(tid,a[3],struct.unpack("I",size)[0]) if size else {"unreadable_socklen":a[4]}
     elif nr==53:e["fds"]=blob(tid,a[3],8)
     elif nr in (22,293):e["fds"]=blob(tid,a[0],8)
     elif nr in (47,299,307):e["unsupported_payload"]=True
    if e["name"]=="UNKNOWN" or e.get("unsupported_payload"):e["complete"]=False
    else:e["complete"]=True
    out.write(json.dumps(e,separators=(",",":"))+"\n")
  elif event == 0 and sig != signal.SIGSTOP:
   out.write(json.dumps({"kind":"signal","tid":tid,"signal":sig,"ir":ir(active[tid]["perf"])})+"\n")
   ptrace(SYSCALL,tid,0,sig);continue
  ptrace(SYSCALL,tid)
 out.write(json.dumps({"kind":"summary","main_rc":main_rc,"memory_failures":failures})+"\n")
 out.close()
 return main_rc if main_rc is not None else 1
if __name__=="__main__":
 sys.exit(main())

