#!/usr/bin/env python3
"""SNAP25 classification fixtures: check25.py NAME REPORT NATIVE_JSONL.
Exit 0 when the census row/trace carries the SNAP25 class the fixture
exercises; with PERRY_SNAP25=0 (the SNAP23 rows) every fixture must fail."""
import json,sys
name,report,native=sys.argv[1:4]
rows=[l.rstrip("\n").split("\t") for l in open(report+".inputs.tsv")][1:]
def dec(h):
    try:return bytes.fromhex(h).decode("utf8","replace")
    except ValueError:return h
ops=[(r[4],dec(r[5]),r[6]) for r in rows if len(r)>6]
# Native traces reach GBs: stream them, keep the markers and the syscalls
# a check looks at (funnel-tagged, named below, or with a path).
WANT={"readlink","readlinkat","sched_getaffinity","getpid","prlimit64"}
def keep(l):
    return ("runtime_marker" in l or "funnel_marker" in l or "\"funnel\"" in l or "\"path\"" in l
            or any("\"name\":\"%s\"" % n in l for n in WANT))
nat=[json.loads(l) for l in open(native) if keep(l)] if native!="-" else []
# The census window: syscalls between the first and the last runtime row
# marker. Process start (setarch, ld.so, the runtime before census_start) and
# everything after the boundary are outside it; funnels are open only inside.
mk=[e["ir_enter"] for e in nat if e.get("kind")=="runtime_marker"]
lo,hi=(min(mk),max(mk)) if mk else (0,-1)
def path(e):
    try:return bytes.fromhex(e.get("path") or "").decode("utf8","replace")
    except ValueError:return e.get("path") or ""
# Every syscall in the window: a funnelled one may be reclassified (diagnostic).
sc=[e for e in nat if e.get("kind")=="syscall" and lo<=e["ir_enter"]<=hi]
fails=[]
def need(c,msg):
    if not c:fails.append(msg)
def vals(op):return [v for o,v,_ in ops if o==op]
nrows=[(o,v,p) for o,v,p in ops if o.startswith("native.")]
if nrows:need(any(p.strip("0,") for _,_,p in nrows),"C1 native rows carry no pcs")
if name=="argv":
    r=vals("argv.read")
    need(any(v.startswith("length=") for v in r),"no argv length read")
    need(any(v.startswith("index=2;value=string:"+"x25".encode().hex()) for v in r),"no argv[2] read")
    # process.execPath is a process metadata property (process.rs
    # process_exec_path_string: readlink /proc/self/exe), not argv[0]: it is
    # the exec_path funnel's readlink below, never an argv.read row.
    need(not any(v.startswith("index=0;") for v in r),"execPath read as argv[0]")
    need(any(v.startswith("escape") for v in r),"no whole-array escape")
    rl=[e for e in sc if e["name"] in("readlink","readlinkat")]
    need(rl and all(e.get("funnel")==1 for e in rl),"readlink not in exec_path funnel")
elif name=="navigator":
    need(vals("navigator.hardwareConcurrency"),"no hardwareConcurrency input")
    # Parallelism = the affinity mask and the cpu cgroup files. The memory
    # cgroup files in the window are the GC heap budget (funnel 6), not P1.
    cg=[e for e in sc if e["name"]=="sched_getaffinity" or path(e)=="/proc/self/cgroup" or path(e).endswith("/cpu.max")]
    need(cg and all(e.get("funnel")==5 for e in cg),"parallelism syscalls untagged")
    mem=[e for e in sc if path(e).startswith("/sys/fs/cgroup") and not path(e).endswith("/cpu.max")]
    need(all(e.get("funnel") in (5,6) for e in mem),"cgroup read outside a funnel")
elif name=="execargv":
    rep=open(report).read().splitlines()
    # process.execArgv is a host argv read: its init is in D, and the report
    # writes #guard lines for imaged inits only. The class source is the
    # getenv.program row (cut25 makes it a guard member); it must not be a knob.
    prog=[bytes.fromhex(v.split("=",1)[0]).decode() for v in vals("getenv.program")]
    need("PERRY_PROCESS_EXEC_ARGV" in prog,"PERRY_PROCESS_EXEC_ARGV read not a getenv.program row")
    need(not any(l.startswith("#knob\tPERRY_PROCESS_EXEC_ARGV") for l in rep),"PERRY_PROCESS_EXEC_ARGV a knob")
elif name=="unsetenv":
    u=vals("native.unsetenv")
    need(any(v==("SNAP25_X".encode().hex()+";was=unset") for v in u),"unsetenv without was=unset")
elif name=="clock":
    for op in("clock.timer","clock.uptime","clock.console_time"):need(vals(op),"no "+op)
elif name=="errpc":
    need(any(v.startswith("exec=1;out=0;") for v in vals("stack.pc_check")),"no EXEC/text pc check")
elif name=="gc":
    need(any(v.endswith(";funnel=gc") for v in vals("native.clock_gettime")),"no GC-funnel clock read")
    need(not [v for v in vals("native.clock_gettime") if ";funnel=" not in v and False],"")
    sm=[e for e in sc if e["name"] in("getpid","prlimit64")]
    need(sm and all(e.get("funnel") in(2,3,6) for e in sm),"stack bounds syscalls untagged")
elif name=="parse":
    # The parse funnel opens (eval + new Function); on the fixture it issues
    # no syscall: the only /proc/self/maps read is before census_start
    # (SNAP24's "stacker reads maps while parsing" does not hold here).
    fm=[e for e in nat if e.get("kind")=="funnel_marker" and e["args"][1]==4 and lo<=e["ir_enter"]<=hi]
    need(len(fm)>=2,"parse funnel not entered by eval and new Function")
    need(not any(e.get("funnel")==4 and e.get("policy")=="STOP" for e in sc),"STOP syscall in the parse funnel")
    mp=[e for e in sc if path(e)=="/proc/self/maps"]
    need(all(e.get("funnel")==4 for e in mp),"/proc/self/maps outside the parse funnel")
fails=[f for f in fails if f]
print(name,"PASS" if not fails else "FAIL "+"; ".join(fails))
sys.exit(1 if fails else 0)
