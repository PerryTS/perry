#!/usr/bin/env python3
"""SNAPK2a census fixtures (DESIGN27 §1.3, §2): checkk2.py NAME REPORT.
Exit 0 when the census rows carry the K2a rule the fixture exercises; with the
rule's knob off (PERRY_SNAP_HOST / PERRY_SNAP_PENTRY / PERRY_SNAP_PREACH = 0)
every fixture must fail."""
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


def need(c, msg):
    if not c:
        fails.append(msg)


def vals(op):
    return [v for _, _, o, v in ops if o == op]


def kv(v):
    return dict(p.split("=", 1) for p in v.split(";") if "=" in p)


host = [kv(v) for v in vals("host_value")]
held = [kv(v) for v in vals("host_value.held")]
pent = [kv(v) for v in vals("p_entry")]
reach = [kv(v) for v in vals("p_reach")]
if name in ("f1", "f10"):
    what = "argv" if name == "f1" else "exec_path"
    need(any(h["what"] == what and h["by"] == "imaged" and h["policy"] == "STOP" for h in host),
         f"no imaged {what} read STOP")
    need(any(h["what"] == what and h["policy"] == "STOP" for h in held), f"no held {what} pointer STOP")
    need(not any(h["by"] == "entry" and h["policy"] != "live" for h in host), "entry read not live")
    if name == "f1":
        need(any(h["what"] == "argv" and h["by"] == "entry" and h["policy"] == "live" for h in host),
             "no live entry argv read")
        need(not any(h.get("holder_init") and h["what"] == "argv" and h["holder_init"] == "0"
                     and h.get("holder") == "slot" for h in held), "entry slot counted as holder")
elif name == "f2":
    for r in "abc":
        need(any(p["rule"] == r and p["policy"] == "STOP" for p in pent), f"no P-entry ({r}) STOP")
    # (a) and (b) name the importing inits, never the entry.
    ab = {p["rule"]: int(p["init"]) for p in pent if p["rule"] in "ab"}
    need(len(set(ab.values())) == len(ab) == 2, f"(a)/(b) not two inits: {ab}")
elif name == "f3":
    need(len(reach) == 2, f"want 2 reach points (b, a), got {len(reach)}")
    if reach:
        first = reach[0].get("members", "")
        seqs = {int(s.split(":")[0]): s.split(":", 1)[1] for s in first.split(",") if ":" in s}
        need(any(op == "argv.read" for op in seqs.values()), f"first reach lacks the argv read: {first}")
        r2 = [v for _, _, o, v in ops if o == "argv.read" and v.startswith("index=2;")]
        need(bool(r2), "no argv[2] index read row")
else:
    fails.append("unknown fixture")
k2a = vals("k2a")
print(f"{name} {'PASS' if not fails else 'FAIL ' + '; '.join(fails)} | k2a={k2a[-1] if k2a else '-'}")
sys.exit(1 if fails else 0)
