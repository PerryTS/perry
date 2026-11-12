# Cut model: prefix cut + blocker ledger from a K2b census run (pty). Versioned here (SNAPREC2); it was /root/SNAPK7/cutk4.py.
# Usage: cut.py V [model]   (V = a | b; reads $SNAP_CUT_DIR/$SNAP_CUT_PFXV-{report,report.inputs.tsv,account.json,native.jsonl} and
#   $SNAP_CUT_DIR/$SNAP_CUT_PFXV.syms; defaults /root/SNAPK7/ana/ and k3; writes k3c-jobs-V.json, cutk3-V[-model].{tsv,json} to $SNAP_CUT_OUT, default the dir)
# SNAPREC2: the entry init(s) and the hasFlag guard init come from the report, not constants; record_settle rows (SNAP_DESIGN 2.10.24):
#   a boundary is a cut point only when every record init that exited has settled EVALUATED; an ASYNC settle is blocker record_async;
#   job_queue with policy=image (a walk root holds the jobs) is no blocker; worker_flag is retired (T0: worker_spawn is the blocker, tls_image accounting).
# Without account.json / native.jsonl (a fixture run) the accounting is the runtime ir stamp itself.
# Self-contained (SNAPK3c): the accounting (realpos/TOT) and resumable-frame cutinfo of SNAP23's cut23.py are inlined.
# Classes: N neutral | M guard member (unset env, compare_literal predicate, style bit, tty) | B blocker (key).
# SNAPK3c: the job_queue rows' jobs= field (kind:owner:code) is grouped by (kind, callback symbol, queueing init, module).
import json,sys,bisect,collections,io,contextlib,csv
import os
V=sys.argv[1]; MODEL=len(sys.argv)>2 and sys.argv[2]=="model"; D=os.environ.get("SNAP_CUT_DIR","/root/SNAPK7/ana/"); P=os.environ.get("SNAP_CUT_PFX","k3")+V; OUT=os.environ.get("SNAP_CUT_OUT",D)
csv.field_size_limit(1<<30)
# --- accounting (cut23): real init Ir up to a runtime ir stamp
rows=list(csv.DictReader(open(D+P+"-report.inputs.tsv"),delimiter="\t"))
for r in rows: r["seq"]=int(r["seq"]); r["ir"]=int(r["ir"]); r["init"]=int(r["init"])
acc=json.load(open(D+P+"-account.json")) if os.path.exists(D+P+"-account.json") else {"real_ids":[],"charges":[],"initial":0,"none":1}
real=set(acc["real_ids"]); ch=acc["charges"]   # [stamp, owner, op, seq], sorted
cum=[]; prev=acc["initial"]; tot=0
for stamp,owner,op,seq in ch:
    if owner in real: tot+=stamp-prev
    cum.append((stamp,tot)); prev=stamp
stamps=[c[0] for c in cum]
def realpos(x):
    i=bisect.bisect_right(stamps,x)-1
    if i<0: return 0
    base=cum[i][1]
    if i+1<len(ch) and ch[i+1][1] in real: base+=min(x,ch[i+1][0])-stamps[i]   # remainder belongs to the next charge's owner
    return base
TOT=tot
if acc.get("none"):
    TOT=max([r["ir"] for r in rows] or [0])
    def realpos(x): return x
if not any(r["op"]=="init_enter" for r in rows) or TOT==0:
    print("no inits")   # SNAPK6: a census that saw no module init has no cut to price
    sys.exit(0)
# --- resumable-frame cut (cut23): last init boundary before stop_seq where every open frame resumes
def parse(sp): return [tuple(map(int,p.split(":"))) for p in sp.split(",") if p]
eff=collections.defaultdict(list)
for r in rows:
    if r["op"] in ("first_changed_tracked_word","native.unsetenv"): eff[r["init"]].append(r["seq"])
def frame_class(fid,phase,seq):
    if phase==1: return "R1"
    bad=[s for s in eff[fid] if s<seq]
    if phase==0: return "R2" if not bad else "none"
    return "R2" if not bad else "none(first effect seq %d)"%bad[0]
ASYNC_OK=[False]   # set while the ledger assumes record_async removed
SETTLE_BY_SEQ={}
def cutinfo(stop_seq):
    best=None; blockers=None; unsettled=set()
    for r in rows:
        if r["seq"]>=stop_seq: break
        if r["op"]=="record_settle":
            i,st=SETTLE_BY_SEQ[r["seq"]]
            if st=="EVALUATED" or ASYNC_OK[0]: unsettled.discard(i)
            continue
        if r["op"] not in ("init_enter","init_exit"): continue
        if r["op"]=="init_exit":
            x=parse(r["stack_parts"])
            if x and x[-1][0] in RECINITS: unsettled.add(x[-1][0])
        if unsettled: continue   # a record body ran and its SCC has not settled EVALUATED: no cut inside a cycle or after a suspension
        st=parse(r["stack_parts"])[:-1]   # drop the entered (init_enter) or exited (init_exit) frame, once (cut23)
        cls=[(f,p,frame_class(f,p,r["seq"])) for f,p,_ in st]
        if all(c.startswith("R") for _,_,c in cls): best=(r,cls)
        else: blockers=(r,cls)
    return best,blockers
byseq={r["seq"]:r for r in rows}
seqs=[r["seq"] for r in rows]
syms=[]
for l in (open(D+P+".syms") if os.path.exists(D+P+".syms") else []):
    f=l.split()
    if len(f)==3:
        try: syms.append((int(f[0],16),f[2]))
        except ValueError: pass
syms.sort(); sa=[s[0] for s in syms]
def sym(pc):
    i=bisect.bisect_right(sa,pc)-1
    return syms[i][1] if i>=0 else "?"
def dec(h):
    try: return bytes.fromhex(h).decode("utf8","replace")
    except ValueError: return h
def kv(v): return dict(p.split("=",1) for p in v.split(";") if "=" in p)
inits={}
for l in open(D+P+"-report"):
    if l.startswith("#"): continue
    f=l.rstrip("\n").split("\t")
    try: inits[int(f[0])]=int(f[1],16)
    except ValueError: pass
iname={a:n[:-len(k)] for a,n in syms for k in ("__init_evaluate","__init_body") if n.endswith(k)}
def mod(i):
    a=inits.get(i); m=iname.get(a,"?%x"%(a or 0))
    for k in("node_modules_","SNAP1_src_"):
        if k in m: m=m.split(k)[-1]
    return m[-55:]
# The entry init(s) from the entry_body row: the init created there is the entry body. A guard-shaped program (no record_settle
# rows) evaluates the entry module as the next init; with records the entry body is the entry's own module body.
SETTLE=[]
for r in rows:
    if r["op"]=="record_settle":
        d=kv(dec(r["value_hex"])); SETTLE.append((r["seq"],int(d.get("init",0)),d.get("status")))
ENTRY=set()
_eb=[r["seq"] for r in rows if r["op"]=="entry_body"]
if _eb:
    # init ids are allocated in enter order: the entry body is the init after the last one entered before its row
    ENTRY.add(1+max([r["init"] for r in rows if r["op"]=="init_enter" and r["seq"]<_eb[0]]+[0]))
    if not SETTLE: ENTRY.add(max(ENTRY)+1)
RECINITS={i for _,i,_ in SETTLE if i and i not in ENTRY}
SETTLE_BY_SEQ.update({s:(i,st) for s,i,st in SETTLE})
# owner 11-08: the argv reads that are has-flag predicates are guard members; the init is the module that calls hasFlag.
def fullname(i): return iname.get(inits.get(i),"")
HASFLAG={i for i in inits if "supports_color" in fullname(i) and fullname(i).endswith("index_js")}
NEUT={"effect","result","stack.payload","stack.capture","stack.rendered","init_enter","init_part","init_exit",
      "job_enter","job_exit","entry_body","census_start","boundary","first_changed_tracked_word","p_reach","open_frames"}
FUN={1:"exec_path",2:"stack_maps",3:"stack_bounds",4:"parse",5:"parallelism",6:"gc",7:"exec_argv",8:"style_bit",9:"unwind"}
NFUN={"stack_maps","stack_bounds","parse","gc","unwind"}
CL=("clock_gettime","__clock_gettime","__vdso_clock_gettime")
unset_seen=set(); ev=[]; EBI=set()   # (ir, seq, op, init, cls, key, detail)
def envread(name,val,init,op):
    if init in ENTRY: return "N",None          # entry body re-runs live
    if val in("unset","<unset>",None): return "M",("env-unset",name)
    if MODEL and name!="USER": return "M",("env-value",name)   # owner 11-06 P1: named env reads guarded; USER = accepted cap (11-08 Q2)
    return "B",("env-value",name)
for r in rows:
    op=r["op"]; v=dec(r["value_hex"]); init=r["init"]; ir=r["ir"]; seq=r["seq"]
    d=kv(v)
    if "at_seq" in d:
        try:
            a=int(d["at_seq"]); i=bisect.bisect_left(seqs,a); ir=rows[min(i,len(rows)-1)]["ir"]; seq=a
        except ValueError: pass
    c,k="B",("op",op)
    if op=="env_block":
        ebi=init or next((x["init"] for x in rows if x["seq"]>seq and x["init"]),0); EBI.add(ebi)
        ev.append((ir,seq,op,init,"B",("env_block",mod(ebi)),v[:120])); continue
    if MODEL and init in EBI and op in("getenv","env_get","env_has"):
        ev.append((ir,seq,op,init,"B",("env_block",mod(init)),v[:120])); continue   # the spread reads every name
    if op in NEUT or op in("initial_environment","k2a"): c,k="N",None
    elif MODEL and init in HASFLAG and (op.startswith("argv") or op=="host_value"): c,k="M",("argv-pred",)   # owner 11-08 Q1 hasFlag predicate guard
    elif op=="record_settle": c,k=("B",("record_async",)) if d.get("status")=="ASYNC" and int(d.get("init",0)) not in ENTRY else ("N",None)
    elif op in("worker_flag","tls_image"): c,k="N",None   # retired (T0) / accounting
    elif op=="worker_spawn": c,k=("N",None) if d.get("policy")=="live" else ("B",("worker_spawn",))
    elif op=="job_queue" and d.get("policy")=="image": c,k="N",None   # the walk root holds the jobs; R1 filters the boundaries
    elif d.get("policy")=="STOP": c,k="B",(op,d.get("what") or d.get("rule") or "")
    elif op in("getenv","getenv.program"):
        n,_,val=v.partition("="); n=dec(n)
        if val=="unset": unset_seen.add(n)
        c,k=envread(n,val,init,op)
    elif op in("env_get","env_has"):
        n=v.split('"')[1] if '"' in v else v[:40]
        val="unset" if ("unset" in v or v.rstrip().endswith(("=0","=None","=false")) or "present=0" in v) else "set"
        c,k=envread(n,val,init,op)
    elif op=="compare_literal": c,k="M",("pred",d.get("src",""))
    elif op=="stdout_style_bit": c,k="M",("stylebit",)
    elif op=="host_value": c,k=("N",None) if d.get("policy")=="live" else ("B",("host_value",d.get("what"),d.get("by")))
    elif op=="tty.isatty": c,k="M",("tty",)
    elif op.startswith("argv"): c,k=("N",None) if init in ENTRY else ("B",("argv",op))
    elif op in("ids.uid","ids.gid"): c,k="B",("ids",)
    elif op=="stack.pc_check": c,k=("N",None) if v.startswith("exec=1;out=0;") else ("B",("pc_check",))
    elif op=="stack.runtime_pc" and d.get("policy")!="STOP": c,k="N",None
    elif op=="native.clock_gettime":
        top=[int(x,16) for x in r["pcs"].split(",") if x not in("","0")]
        ss=[x for x in (sym(p) for p in top) if x not in CL]; s0=ss[0] if ss else "?"
        fun=v.rsplit(";funnel=",1)[1] if ";funnel=" in v else None
        if fun in NFUN or s0.startswith(("mi_","_mi_")): c,k="N",None
        else: c,k="B",("clock",s0[:40])
    elif op=="native.unsetenv":
        name=dec(v.split(";")[0])
        c,k=("N",None) if (";was=unset" in v and name in unset_seen) else ("B",("unsetenv",name))
    ev.append((ir,seq,op,init,c,k,v[:120]))
# native STOP syscalls (tracer domain -> runtime domain via markers)
mk=[];nat=[]
for l in (open(D+P+"-native.jsonl") if os.path.exists(D+P+"-native.jsonl") else []):
    if '"runtime_marker"' not in l and '"STOP"' not in l: continue
    x=json.loads(l)
    if x.get("kind")=="runtime_marker" and x.get("runtime_seq") in byseq: mk.append((x["ir_enter"],x["ir_enter"]-byseq[x["runtime_seq"]]["ir"]))
    elif x.get("kind")=="syscall" and x.get("policy")=="STOP" and x.get("name")!="clock_gettime": nat.append(x)
mk.sort(); mi=[m[0] for m in mk]
irs=[r["ir"] for r in rows]
for x in nat:
    i=bisect.bisect_right(mi,x["ir_enter"])-1
    if i<0: continue
    ri=x["ir_enter"]-mk[i][1]
    fun=FUN.get(x.get("funnel",0)); nm=x["name"]
    path=dec(x["path"]) if "path" in x else ""
    init=rows[max(0,bisect.bisect_right(irs,ri)-1)]["init"]
    if fun in NFUN or fun in("exec_path","exec_argv"): c,k="N",None
    elif fun=="parallelism": c,k="B",("cpus",)
    elif nm=="ioctl": c,k="M",("tty",)
    elif nm in("getuid","getgid","geteuid","getegid"): c,k="B",("ids",)
    else: c,k="B",("syscall",nm)
    pcs=x.get("pcs_scan") or []
    ev.append((ri,-1,"nat:"+nm,init,c,k,(path[:50]+" "+(sym(pcs[0])[:50] if pcs else ""))))

# --- SNAPK3c: identity of the pending jobs at each job_queue row
def jobs_of(v):
    d=kv(v); out=[]
    for j in d.get("jobs","").split(","):
        if not j or j.startswith("+") or j=="busy": continue
        k,o,c=j.split(":"); out.append((k,int(o),int(c,16)))
    return d,out
openf={}   # init -> open frame inits at its init_enter (from the open_frames row)
for r in rows:
    if r["op"]=="open_frames":
        d=kv(dec(r["value_hex"])); openf[int(d.get("init",0))]=[int(x.split(":")[0]) for x in d.get("open","").split(",") if x]
grp=collections.OrderedDict(); jqrows=0; nopend=collections.Counter()
for r in rows:
    if r["op"]!="job_queue": continue
    jqrows+=1; d,js=jobs_of(dec(r["value_hex"])); bi=int(d.get("init",0)); nopend[d.get("pending")]+=1
    for k,o,c in js:
        g=(k,sym(c) if c else "(no callback: settles next)",o,mod(o))
        e=grp.setdefault(g,{"rows":0,"first":bi,"last":bi,"open_owner":0,"ir_first":r["ir"]})
        e["rows"]+=1; e["last"]=bi; e["open_owner"]+=o in openf.get(bi,[])
print("K3C job_queue rows:",jqrows,"pending counts:",dict(nopend))
print("K3C pending jobs grouped by (kind, callback, queueing init, module):")
print("  %-9s %-60s %6s %-50s %6s %6s %6s %s"%("kind","callback","owner","module","rows","first","last","owner_open"))
jout=[]
for (k,s,o,m),e in grp.items():
    print("  %-9s %-60s %6d %-50s %6d %6d %6d %d  first_at=%.2f%%"%(k,s[:60],o,m[:50],e["rows"],e["first"],e["last"],e["open_owner"],100*realpos(e["ir_first"])/TOT))
    jout.append({"kind":k,"callback":s,"owner":o,"module":m,**e})
json.dump(jout,open(OUT+"k3c-jobs-%s.json"%V,"w"),indent=1)
print("unwind-funnel rows:",sum(1 for r in rows if "funnel=unwind" in dec(r["value_hex"])), "native.clock_gettime rows:",sum(1 for r in rows if r["op"]=="native.clock_gettime"))
ev.sort(key=lambda e:(e[0],e[1]))
for e in ev:
    if e[2].startswith("nat:") and e[4]=="B": print("  NATB ir=%d pct=%.2f %s init %d %s %s"%(e[0],100*realpos(e[0])/TOT,e[2],e[3],mod(e[3]),e[6][:90]))
def stop_seq_of(e):
    if e[1]>=0: return e[1]
    i=bisect.bisect_left(irs,e[0]); return rows[min(i,len(rows)-1)]["seq"]
def first_block(removed):
    for e in ev:
        if e[4]=="B" and e[5] not in removed: return e
    return None
def cut(e):
    s=stop_seq_of(e) if e else rows[-1]["seq"]+1
    with contextlib.redirect_stdout(io.StringIO()): best,bl=cutinfo(s)
    if best is None: return {"seq":-1,"init":-1,"ir":0},0   # blocker before the first resumable init boundary
    r,cls=best; return r,realpos(r["ir"])
out={"V":V,"TOT":TOT,"ledger":[]}
print("run",V,"real_init_Ir",TOT,"rows",len(rows),"events",len(ev))
print("class counts:",collections.Counter(e[4] for e in ev))
removed=set(); prev=None
print("first B events:")
for e in [x for x in ev if x[4]=="B"][:12]: print("  B ir=%d seq=%d %s init %d %s %s | %s"%(e[0],e[1],e[2],e[3],mod(e[3]),e[5],e[6][:100]))
for step in range(60):
    e=first_block(removed); r,rp=cut(e)
    pct=100*rp/TOT
    if prev is not None: out["ledger"][-1]["gain_pct"]=round(pct-prev,3)
    if e is None:
        print("END no more blockers: cut at seq",r["seq"],"init",r["init"],"%.2f%%"%pct); out["final"]=pct; break
    n=sum(1 for x in ev if x[5]==e[5])
    ninit=len({x[3] for x in ev if x[5]==e[5]})
    print("%2d cut %6.2f%% before init %4d | blocker %-40s first: %s init %d %s | %s | rows %d inits %d"%(step,pct,r["init"],str(e[5])[:40],e[2],e[3],mod(e[3]),e[6][:90],n,ninit))
    out["ledger"].append({"step":step,"cut_pct":round(pct,3),"cut_before_init":r["init"],"key":list(map(str,e[5])),"op":e[2],"init":e[3],"module":mod(e[3]),"detail":e[6],"rows":n,"inits":ninit})
    removed.add(e[5]); prev=pct; ASYNC_OK[0]=("record_async",) in removed
# guard members inside the first cut
e0=first_block(set()); r0,_=cut(e0)
mem=collections.Counter(e[5] for e in ev if e[4]=="M" and e[1]<r0["seq"] and e[1]>=0)
print("guard members before the first cut:",sorted(mem.items(),key=str)[:40])
out["members_first_cut"]=[[list(map(str,k)),n] for k,n in mem.items()]
ops=collections.Counter(r["op"] for r in rows)
print("ops:",dict(ops.most_common(45)))
with open(OUT+"cutk3-%s%s.tsv"%(V,"-model" if MODEL else ""),"w") as t:
    t.write("step\tcut_pct\tgain_pct\tcut_before_init\tkey\top\tinit\tmodule\trows\tinits\tdetail\n")
    for L in out["ledger"]: t.write("\t".join(str(L.get(c,"")) for c in ("step","cut_pct","gain_pct","cut_before_init","key","op","init","module","rows","inits","detail"))+"\n")
bk=collections.Counter(e[5] for e in ev if e[4]=="B")
print("B keys total:",len(bk)); print("top B keys by rows:",bk.most_common(30))
json.dump(out,open(OUT+"cutk3-%s%s.json"%(V,"-model" if MODEL else ""),"w"),indent=1)
# guard members inside the product cut (owner-accepted USER cap: worker, env_block fromEnv, job_queue, ids removed)
rm4=set(tuple(map(str,L["key"])) for L in out["ledger"][:4])
e4=next((e for e in ev if e[4]=="B" and tuple(map(str,e[5])) not in rm4),None); r4,p4=cut(e4)
m4=collections.Counter(e[5] for e in ev if e[4]=="M" and 0<=e[1]<r4["seq"])
print("PRODUCT cut %.2f%% before init %d; guard members %d keys:"%(100*p4/TOT,r4["init"],len(m4)),sorted(m4.items(),key=str))
