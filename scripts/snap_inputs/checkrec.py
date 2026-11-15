#!/usr/bin/env python3
"""SNAPREC2 census fixtures (SNAP_DESIGN 2.10.24 / 2.10.22 T0): checkrec.py NAME REPORT [STDOUT].
NAME = f8 positive-control | f9 entry-moved | f6n f9n no-hold | f1 sync | f2 import-job | f3 tla-dep | f4 cycle | f5 worker | f6 stale-holder | f7 stale-target
(scripts/snap_inputs/fixtures/rec/NAME).
Reads the census rows (record_settle, job_queue walk_root, worker_spawn, tls_image) and runs the cut model (cut.py)
on the report: exit 0 when the rows and the cut are what the design states."""
import json, os, re, subprocess, sys, tempfile
name, report = sys.argv[1:3]
stdout = open(sys.argv[3]).read() if len(sys.argv) > 3 else None
F7_HELD_WORDS = 0  # measured: b holds no entry object once only GC-traced slots are scanned
HERE = os.path.dirname(os.path.abspath(__file__))


def dec(h):
    try:
        return bytes.fromhex(h).decode("utf8", "replace")
    except ValueError:
        return h


def kv(v):
    return dict(p.split("=", 1) for p in v.split(";") if "=" in p)


rows = [l.rstrip("\n").split("\t") for l in open(report + ".inputs.tsv")][1:]
ops = [dict(seq=int(r[0]), init=int(r[2]), stack=r[3], op=r[4], v=dec(r[5])) for r in rows if len(r) > 6]
rec = {}
for l in open(report):
    if l.startswith("#record\t"):
        _, a, i = l.rstrip("\n").split("\t")
        rec[a] = int(i)
fails = []


def need(c, msg):
    if not c:
        fails.append(msg)


def of(op):
    return [o for o in ops if o["op"] == op]


need(of("boundary"), "no boundary row (the run never reached gc())")
eb = of("entry_body")
E = 1 + max([o["init"] for o in of("init_enter") if eb and o["seq"] < eb[0]["seq"]] + [0])
settle = [dict(o, **kv(o["v"])) for o in of("record_settle")]
for s in settle:
    s["init"] = int(s["init"])
body = [s for s in settle if s["init"] not in (0, E)]
# the cut model on this report (no account/native files: the accounting is the ir stamp)
tmp = tempfile.mkdtemp()
os.symlink(os.path.abspath(report), f"{tmp}/rec{name}-report")
os.symlink(os.path.abspath(report + ".inputs.tsv"), f"{tmp}/rec{name}-report.inputs.tsv")
env = dict(os.environ, SNAP_CUT_DIR=tmp + "/", SNAP_CUT_PFX="rec", SNAP_CUT_OUT=tmp + "/")
p = subprocess.run([sys.executable, f"{HERE}/cut.py", name, "model"], env=env, capture_output=True, text=True)
need(p.returncode == 0, "cut.py failed: " + p.stderr[-200:])
cj = json.load(open(f"{tmp}/cutk3-{name}-model.json")) if p.returncode == 0 else {"ledger": []}
led = cj["ledger"]
end = re.search(r"END no more blockers: cut at seq (\d+) init (\d+)", p.stdout)
cut_end = int(end.group(2)) if end else None
keys = [L["key"][0] for L in led]
info = f"entry={E} settle={[(s['init'], s['status']) for s in settle]} ledger={[(L['key'][0], L['cut_before_init']) for L in led][:4]} end_init={cut_end}"
if name == "f1":
    need(len(body) == 2 and all(s["status"] == "EVALUATED" for s in body), f"want two EVALUATED settle rows, got {len(body)}")
    need(not any(s["status"] == "ASYNC" for s in body), "an ASYNC settle of a dependency in a program without suspension")
    need("record_async" not in keys, "record_async blocker")
    need(body and cut_end is not None and cut_end >= max(s["init"] for s in body), "the cut is not after b")
elif name == "f2":
    jq = [kv(o["v"]) for o in of("job_queue")]
    imaged = [j for j in jq if j.get("policy") == "image" and int(j.get("walk_root", "0") or "0", 16) != 0]
    need(imaged, f"no job_queue row with a walk_root (rows={jq[:2]})")
    if imaged:
        j = imaged[0]
        root = rec.get(j["walk_root"])
        ini = [o["init"] for o in of("init_enter")]
        need(root == max(ini), f"walk_root init {root} is not the last-entered init {max(ini)} (sub)")
        need(int(j["init"]) < (root or 0), "the row is not at an init below the root")
        first = j["jobs"].split(",")[0].split(":")
        need(first[0] in ("promise", "inline") and int(first[1]) == int(j["init"]) - 1, f"the pending job is not a's: {j['jobs']}")
    need("job_queue" not in keys, "job_queue is still a blocker")
elif name == "f3":
    a = [s for s in body if s["status"] == "ASYNC"]
    need(len(a) >= 1, "no ASYNC settle row (t)")
    r = [L for L in led if L["key"][0] == "record_async"]
    need(r and a and r[0]["cut_before_init"] == a[0]["init"], f"the cut is not before t: {r[:1]} vs {a[:1]}")
elif name == "f4":
    ex = sorted(o["seq"] for o in of("init_exit") if o["init"] in {s["init"] for s in body})
    need(len(body) == 2 and all(s["status"] == "EVALUATED" for s in body), "want two EVALUATED settle rows")
    need(len(ex) == 2 and all(s["seq"] > ex[1] for s in body), "a member settled before the cycle's last body exited")
    need("record_async" not in keys, "record_async blocker")
    # no cut candidate between the first member's exit and the last member's settle
    need(end is not None and len(ex) == 2 and not (ex[0] < int(end.group(1)) < max(s["seq"] for s in body)), "the cut lies inside the cycle")
elif name == "f5":
    need(not of("worker_flag"), "a worker_flag row")
    # the Worker starts after gc() (the boundary): the census is off then, so there is no worker_spawn row
    need(not of("worker_spawn"), "a worker_spawn row for a worker started after the cut")
    tl = [kv(o["v"]) for o in of("tls_image")]
    need(len(tl) == 1 and int(tl[0].get("tls_ptr_words", "0")) > 0 and int(tl[0].get("data", "0")) + int(tl[0].get("bss", "0")) > 0, f"tls_image row: {tl}")
    need("worker_spawn" not in keys, "worker_spawn is a blocker")
    bs = of("boundary")
    need(bs and all(o["seq"] > bs[0]["seq"] for o in of("tls_image")), "the tls_image row is not after the boundary row (its scan is charged to an init)")
    info += f" tls_image={tl}"
    need(stdout is None or "worker evals=101 tid=1" in stdout, "the worker did not print evals=101 tid=1")
elif name in ("f6", "f7", "f8", "f9", "f6n", "f9n"):
    # Hold rows (rule c, held host values) come from a run that holds collection only:
    # f6 f7 f8 f9 run held; f6n f9n run with PERRY_SNAP_NOPROTECT=1 and must say they collected no holds.
    k2a = [kv(o["v"]) for o in of("k2a")]
    need(len(k2a) == 1, f"k2a rows: {k2a}")
    k2a = k2a[0] if k2a else {}
    pc = [dict(o, **kv(o["v"])) for o in of("p_entry") if o["v"].startswith("rule=c;")]
    hv = [o for o in of("host_value.held")]
    info += f" holds={k2a.get('holds')} held_words={k2a.get('held_words')} rule_c_rows={len(pc)}"
    if name.endswith("n"):
        need(k2a.get("holds") == "not-collected(no-hold)", f"no not-collected marker: {k2a}")
        need(not pc and not hv, f"{len(pc)} rule-c and {len(hv)} held-value rows from a run without the collection hold")
        need(int(k2a.get("stale_births", "0")) > 0, "no memory was reused: the run is not a moving run")
    else:
        need(k2a.get("holds") == "collected", f"holds marker: {k2a}")
        # the entry record is imaged in configure state: none of its root slots is a holder
        inrec = [p for p in pc if p.get("holder") == "slot" and p.get("holder_init") == "0"]
        need(not inrec, f"{len(inrec)} slot rows from owner 0 (the entry record's body_promise): {[p.get('slot') for p in inrec[:3]]}")
        ent = of("init_enter")
        b = max(o["init"] for o in ent)
        need(b > E, f"last init ({b}) is not entered after the entry ({E})")
        words = [int(p["holder_addr"], 16) + int(p["off"]) for p in pc if "holder_addr" in p]
        need(len(words) == len(set(words)), f"{len(words) - len(set(words))} words read by two holders")
        info += f" last_init={b}"
        if name == "f6":
            need(sum(int(p.get("holder_init", "-1")) == b for p in pc) >= 1000, "b's holds of E are not reported")
        if name == "f7":
            need(int(k2a.get("held_words", "-1")) == F7_HELD_WORDS, f"held_words {k2a.get('held_words')} != {F7_HELD_WORDS}")
        if name == "f8":
            # an imaged dependency's module-level let holds an entry object: still rule (c)
            sl = [p for p in pc if p.get("holder") == "slot" and p.get("holder_init") not in ("0", str(E))]
            need(len(sl) >= 1, "the dependency's slot holding an entry object is not reported")
        if name == "f9":
            mine = [p for p in pc if int(p.get("holder_init", "-1")) == b]
            need(len(mine) == 1 and mine[0].get("holder") == "birth", f"rows held by the last module: {len(mine)}")
else:
    fails.append("unknown fixture")
# a P-entry (a)/(b) row whose pc lies in the census itself is a census read charged to an init
binp = os.path.join(os.path.dirname(os.path.abspath(report)), "bin")
if os.path.exists(binp):
    syms = []
    for l in subprocess.run(["nm", "-n", "--defined-only", binp], capture_output=True, text=True).stdout.splitlines():
        f = l.split(None, 2)
        if len(f) == 3 and f[1] in "tTwW":
            syms.append((int(f[0], 16), f[2]))
    import bisect
    own = []
    for o in of("p_entry"):
        k = kv(o["v"])
        if k.get("rule") in ("a", "b") and syms:
            j = bisect.bisect_right([x[0] for x in syms], int(k["pc"], 16)) - 1
            if j >= 0 and "snap_census" in syms[j][1]:
                own.append((k["rule"], k["init"], syms[j][1][-48:]))
    need(not own, f"rule (a)/(b) rows read by the census itself: {own[:3]}")
print(f"{name} {'PASS' if not fails else 'FAIL ' + '; '.join(fails)} | {info}"[:900])
sys.exit(1 if fails else 0)
