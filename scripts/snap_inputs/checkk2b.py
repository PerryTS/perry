#!/usr/bin/env python3
"""SNAPK2b census fixtures (DESIGN27 §1.1, §1.2, §1.5, §3): checkk2b.py NAME REPORT.
Exit 0 when the census rows carry the K2b rule the fixture exercises. Each
fixture's rule knob (PERRY_SNAP_CMPLIT, _STYLE, _OPENF, _JOBQ, _RTPC, _WORKER,
_PENTRY, _ENV) off must make it fail. NAME variants pick the expected value:
f5s/f5l (TERM=screen/linux), f13 (on: rt=0) and f13s (PERRY_SNAP_RTPC_JS=none:
STOP)."""
import os
import sys
name, report = sys.argv[1:3]
rows = [l.rstrip("\n").split("\t") for l in open(report + ".inputs.tsv")][1:]


def dec(h):
    try:
        return bytes.fromhex(h).decode("utf8", "replace")
    except ValueError:
        return h


ops = [(int(r[0]), int(r[2]), r[4], dec(r[5])) for r in rows if len(r) > 6]
fails = []
if not any(o == "boundary" for _, _, o, _ in ops):
    fails.append("no boundary row (the run never reached gc())")


def need(c, msg):
    if not c:
        fails.append(msg)


def vals(op):
    return [v for _, _, o, v in ops if o == op]


def kv(v):
    return dict(p.split("=", 1) for p in v.split(";") if "=" in p)


def inits(op, pred=lambda v: True):
    return {i for _, i, o, v in ops if o == op and pred(v)}


def entry_init():
    # The fixture's entry reads SNAPK2B_ENTRY; that row's init is the entry.
    e = inits("env_get", lambda v: v.startswith('"SNAPK2B_ENTRY"'))
    return next(iter(e)) if len(e) == 1 else None


cl = [kv(v) for v in vals("compare_literal")]
info = ""
if name == "f3n":
    reach = [kv(v) for v in vals("p_reach")]
    need(len(reach) == 1, f"want 1 reach point (a.ts), got {len(reach)}")
    argv_cl = [c for c in cl if c["src"].startswith("argv@")]
    need(len(argv_cl) == 2 and all(c["result"] == "0" and c["by"] == "entry" for c in argv_cl),
         f"want two false entry argv compare_literal rows, got {argv_cl}")
    need({dec(c["literal"]) for c in argv_cl} == {"--host-a", "--host-b"}, "sentinels not the literals")
    if reach:
        r = reach[0]
        members = [m.split(":", 1)[1] for m in r.get("members", "").split(",") if ":" in m]
        info = f"argv={r.get('argv')} members={members}"
        need(r.get("argv") == "predicate", f"argv escape not narrowed: argv={r.get('argv')}")
        need(members.count("compare_literal.argv") == 2, "members lack the two predicates")
        need(not any(m in ("argv.escape", "host_value.argv") for m in members), "argv escape still a member")
elif name in ("f5s", "f5l"):
    want = "1" if name == "f5s" else "0"
    env = [c for c in cl if c["src"] == "env:TERM"]
    info = f"rows={env}"
    need(len(env) == 1, f"want one TERM compare_literal row, got {len(env)}")
    need(all(dec(c["literal"]) == "linux" and c["result"] == want and c["by"] == "imaged" for c in env),
         f"TERM row not literal=linux result={want} by=imaged")
elif name == "f6":
    sb = [kv(v) for v in vals("stdout_style_bit")]
    info = f"rows={sb}"
    need(len(sb) == 1, f"want one stdout_style_bit row, got {len(sb)}")
    need(all(r["bit"] == "0" and r["tty"] == "0" and r["by"] == "imaged" for r in sb),
         "style row not bit=0 tty=0 by=imaged")
elif name == "f7":
    of = [kv(v) for v in vals("open_frames")]
    info = f"rows={of}"
    both = [r for r in of if {k.split(":")[1] for k in r["open"].split(",")} == {"R1", "R2"}]
    need(len(both) == 1 and both[0]["policy"] == "resume" and both[0]["flags"] == "0",
         "no resume open_frames row with an R1 and an R2 ancestor")
    need(not any(r["policy"] == "STOP" for r in of), "open_frames STOP")
elif name == "f12":
    jq = [kv(v) for v in vals("job_queue")]
    info = f"rows={jq}"
    need(len(jq) >= 1 and all(r["policy"] == "STOP" for r in jq), "no job_queue STOP row")
    # SNAPK3c: each row names its jobs (kind:owner:code); f12's one job is the
    # m.ts reaction, queued by an init before the row's, with a callback.
    for r in jq:
        js = [j.split(":") for j in r.get("jobs", "").split(",") if j and not j.startswith("+")]
        need(r.get("pending") == str(len(js)), "job_queue jobs= does not list every pending job")
        need(all(len(j) == 3 and int(j[1]) not in (0, int(r["init"])) and int(j[2], 16) != 0 for j in js),
             "a job without its queueing init or callback")
elif name in ("f13", "f13s"):
    rp = [kv(v) for v in vals("stack.runtime_pc")]
    m_init = inits("stack.runtime_pc")
    info = f"rows={rp}"
    need(len(rp) >= 1, "no stack.runtime_pc row")
    # An empty capture classifies nothing: the arm must hold JS frames.
    need(all(int(r["n"]) > 0 for r in rp), "a capture with no frames")
    if name == "f13":
        need(all(r["rt"] == "0" and r["policy"] == "symbol" for r in rp), "a runtime pc in a JS stack")
    else:
        need(all(int(r["rt"]) > 0 and r["policy"] == "STOP" for r in rp), "no runtime-pc STOP")
    need(0 not in m_init and all(r["init"] != "0" for r in rp), "capture outside an init")
elif name == "fw":
    wf = [kv(v) for v in vals("worker_flag")]
    info = f"rows={wf}"
    need(len(wf) == 1 and wf[0]["cut"] == "0" and wf[0]["policy"] == "STOP", "no worker_flag STOP row")
elif name == "f2c":
    e = entry_init()
    c = [kv(v) for v in vals("p_entry") if kv(v).get("rule") == "c"]
    info = f"entry={e} c=" + ",".join(f"{p.get('holder_init')}/{p.get('holder_type')}@{p.get('off')}" for p in c)
    need(e is not None, "no entry env row")
    need(len(c) == 1 and c[0]["policy"] == "STOP", f"want one (c) STOP holder, got {len(c)}")
    need(all(p.get("holder_init") not in (None, "0", str(e)) for p in c), "(c) holder not the eager init")
elif name == "f4":
    lines = open(report).read().splitlines() + [l for l in open(report + ".inputs.tsv")]
    g = [l.split("\t") for l in lines if l.startswith("#guard\tSNAPK2B_UNSET\t")]
    info = f"guards={g}"
    need(any(x[2] == "<unset>" and x[5] == "store_unset" for x in g), "no store_unset guard for SNAPK2B_UNSET")
elif name == "f8":
    e = entry_init()
    u = inits("ids.uid")
    info = f"entry={e} uid_inits={u}"
    need(e is not None and len(u) == 1 and not (u & {0, e}), "ids.uid not read by the imaged init")
else:
    fails.append("unknown fixture")
print(f"{name} {'PASS' if not fails else 'FAIL ' + '; '.join(fails)} | {info}"[:600])
sys.exit(1 if fails else 0)
