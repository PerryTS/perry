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
elif name in ("f12t", "f12tj"):
    # SNAPK6: a program with top-level await evaluates every module as an
    # async module record; each body is one census init, reached statically
    # (reach 0/1, not dynamic) whatever job runs it, and the entry body opens
    # at the entry record's body, after the dependencies.
    lines = open(report).read().splitlines()
    head = dict(p.split("=", 1) for p in lines[0].split("\t") if "=" in p)
    hdr = next(i for i, l in enumerate(lines) if l.startswith("#id\t"))
    cols = ["id"] + lines[hdr].split("\t")[1:]
    idrows = []
    for l in lines[hdr + 1:]:
        if l.startswith("#"):
            break
        idrows.append(dict(zip(cols, l.split("\t"))))
    mods = [r for r in idrows if r["id"] != "0" and r["addr"] != "1"]
    entry = [r for r in idrows if r["addr"] == "1"]
    want = 3 if name == "f12t" else 4
    info = f"head={ {k: head.get(k) for k in ('seq', 'entry_body_seq', 'inits', 'w')} } mods={[(r['id'], r['reach'], r['dynamic']) for r in mods]}"
    need(len(mods) == want and len({r["addr"] for r in mods}) == want, f"want {want} distinct module inits, got {len(mods)}")
    need(all(r["enter"] != "0" for r in mods), "a module init without its enter")
    need(head.get("inits") == str(want + 1), f"inits={head.get('inits')}, want {want + 1}")
    # w = write-faulted ranges: 0 without a timer (f12t); a timer (f12tj) writes its own state.
    need(name != "f12t" or head.get("w") == "0", f"w={head.get('w')}, want 0")
    need(not vals("worker_flag"), "a worker_flag row in a program without a worker")
    need(all(r["reach"] in ("0", "1") and r["dynamic"] == "0" for r in mods),
         "a static dependency is not EntryEager/StaticDep")
    need(all(r["exit"] != "0" and r["open"] == "0" and r["threw"] == "0" for r in mods), "a module init not closed")
    need(len(entry) == 1 and entry[0]["open"] == "1" and entry[0]["from_entry_body"] == "1", "no open entry body")
    if entry and mods:
        need(all(int(entry[0]["enter"]) > int(r["exit"]) for r in mods), "entry body opened before a dependency exited")
        need(head.get("entry_body_seq") == entry[0]["enter"], "entry_body_seq is not the entry body's enter")
elif name == "f14q":
    # SNAPK4: the run queues beside TASK_QUEUE, each named from its own store.
    # m.ts queues a nextTick, a util.aborted resolution, a timeout (25), an
    # interval (40) and an immediate; n.ts's init_enter sees all of them.
    def ents(rows, field):
        return [e.split(":") for r in rows for e in r.get(field, "").split(",") if e and not e.startswith("+")]
    tq = [kv(v) for v in vals("tick_queue")]
    sr = [kv(v) for v in vals("scheduled_resolves")]
    tm = [kv(v) for v in vals("timer_queue")]
    info = f"tick={tq[:1]} resolves={sr[:1]} timers={tm[:1]}"
    need(len(tq) >= 1 and all(r["policy"] == "image" for r in tq), "no tick_queue image row")
    need(all(len(e) == 3 and e[0] == "tick" and int(e[1]) != 0 and int(e[2], 16) != 0 for e in ents(tq, "ticks")),
         "a tick without its queueing init or callback")
    need(len(sr) >= 1 and all(r["policy"] == "image" for r in sr), "no scheduled_resolves image row")
    need(all(len(e) == 3 and e[0] == "resolve" and int(e[1]) != 0 for e in ents(sr, "resolves")),
         "a scheduled resolve without its queueing init")
    need(len(tm) >= 1 and all(r["policy"] == "STOP" for r in tm), "no timer_queue STOP row")
    te = ents(tm, "timers")
    need(all(len(e) == 5 and int(e[1]) != 0 for e in te), "a timer without its queueing init")
    kinds = {(e[0], e[3], e[4]) for e in te if int(e[2], 16) != 0}
    for k in (("timeout", "25", "0"), ("interval", "40", "1"), ("immediate", "0", "0")):
        need(k in kinds, f"no {k[0]} entry with delay {k[1]} repeat {k[2]} and a callback")
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
    # SNAPREC2 (SNAPTLS1 T0): no static worker_flag STOP; the primary agent's TLS block is imaged (tls_image at the boundary).
    tl = [kv(v) for v in vals("tls_image")]
    info = f"tls_image={tl}"
    need(not vals("worker_flag"), "a worker_flag row (retired)")
    need(len(tl) == 1 and tl[0]["policy"] == "image", "no tls_image row")
    need(not [v for v in vals("worker_spawn") if kv(v)["policy"] == "STOP"], "a worker_spawn STOP row in a program that starts no worker")
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
elif name == "f15u":
    # SNAPK7: throws from 600 distinct sites with allocator churn. A throw
    # decodes call-frame rows under the walker lock; an allocation there can
    # read the clock (a census native row), which must not walk the stack
    # again under that lock. Red: the run deadlocks, no boundary, no report.
    need(os.path.exists(report), "no report (the run never finished)")
    info = f"rows={len(ops)} boundary={sum(1 for _, _, o, _ in ops if o == 'boundary')}"

elif name in ("envheld", "envheld-enum"):
    # process.env holds no host data until enumerated: iso.ts only holds the
    # reference during init (and the entry reads one key after the cut), so no
    # env_block row; envheld-enum has enum.ts call Object.keys(process.env) in
    # its own init, so the block is read there and only there.
    eb = [(i, s) for s, i, o, _ in ops if o == "env_block"]
    ie = [i for _, i, o, _ in sorted(ops) if o == "init_enter"]
    info = f"env_block={eb} init_enters={ie}"
    if name == "envheld":
        need(len(eb) == 0, f"want 0 env_block rows, got {len(eb)}")
    else:
        need(len(ie) >= 3, "want the three module inits")
        need(len(eb) >= 1 and (len(ie) < 3 or {i for i, _ in eb} == {ie[1]}),
             "env_block rows must come only from the enumerating init (enum.ts, the second)")
else:
    fails.append("unknown fixture")
print(f"{name} {'PASS' if not fails else 'FAIL ' + '; '.join(fails)} | {info}"[:600])
sys.exit(1 if fails else 0)
