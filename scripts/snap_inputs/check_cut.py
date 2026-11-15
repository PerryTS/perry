#!/usr/bin/env python3
"""Model tests for cut.py on synthetic censuses (no compile, no census run): python3 check_cut.py [cut.py].
Each case writes a tiny report + inputs.tsv, runs `cut.py x model` and asserts the ledger, the PRODUCT stop and the boundary.
Exit 0 when every case holds."""
import json, os, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
CUT = sys.argv[1] if len(sys.argv) > 1 else HERE + "/cut.py"
J = "4:0:1"   # the job frame under which every module body runs


def row(seq, ir, init, stack, op, v=""):
    return [str(seq), str(ir), str(init), stack, op, v.encode().hex(), "", "0"]


def build(spec):
    """spec: list of (init, stack_parts, [(op, value)...]) in enter order; each body is enter, ops, exit.
    Returns rows with ir = 100 * seq so the percentages are exact."""
    rows, seq = [], 0

    def add(init, stack, op, v=""):
        nonlocal seq
        seq += 1
        rows.append(row(seq, 100 * seq, init, stack, op, v))
    add(0, "", "census_start")
    add(0, "", "boundary")
    for init, stack, ops in spec:
        add(init, stack, "init_enter", "x")
        for op, v in ops:
            add(init, stack, op, v)
        add(init, stack, "init_exit", "threw=false")
    return rows


def run(spec):
    d = tempfile.mkdtemp()
    rows = build(spec)
    with open(d + "/tx-report.inputs.tsv", "w") as f:
        f.write("seq\tir\tinit\tstack_parts\top\tvalue_hex\tpcs\tallocated_bytes\n")
        for r in rows:
            f.write("\t".join(r) + "\n")
    with open(d + "/tx-report", "w") as f:
        f.write("#id\taddr\n")
        for i in sorted({int(r[2]) for r in rows if int(r[2])}):
            f.write("%d\t%x\n" % (i, 0x1000 + i))
    env = dict(os.environ, SNAP_CUT_DIR=d + "/", SNAP_CUT_PFX="t", SNAP_CUT_OUT=d + "/")
    p = subprocess.run([sys.executable, CUT, "x", "model"], env=env, capture_output=True, text=True)
    if p.returncode:
        return None, p.stdout, p.stderr[-300:], rows
    return json.load(open(d + "/cutk3-x-model.json")), p.stdout, "", rows


fails = []


def need(c, msg):
    if not c:
        fails.append(msg)


def body(n):
    return (n, "%s,%d:0:0" % (J, n), [])


# 1. uid stop: modules 1..4, uid at 3; TERM member at 4. The product stops at 3 with a reason; the what-if passes it.
j, out, err, rows = run([body(1), body(2), (3, J + ",3:0:0", [("ids.uid", "()")]), (4, J + ",4:0:0", [("getenv", "TERM=xterm")])])
need(j is not None, "case1: cut.py failed " + err)
if j:
    need(j["product"]["stop"] == ["ids"] and j["product"]["cut_before_init"] == 3, "case1: product %s" % j["product"])
    need("STOP ('ids',)" in out and "no startup guard member exists" in out, "case1: the stop reason is not printed")
    need(j["whatif"] and j["whatif"][0]["removed"] == [["ids"]], "case1: what-if missing")
    need("HYPOTHETICAL" in out and "WHAT-IF" in out, "case1: what-if not labelled")
# 2. USER stop: the cut stays before the init that copies USER; TERM would be a member.
j, out, err, rows = run([body(1), (2, J + ",2:0:0", [("getenv", "TERM=xterm")]), (3, J + ",3:0:0", [("getenv", "USER=root")])])
need(j is not None, "case2: cut.py failed " + err)
if j:
    need(j["product"]["stop"] == ["env-value", "USER"] and j["product"]["cut_before_init"] == 3, "case2: product %s" % j["product"])
    need(any("env-value" in t for t in out.splitlines() if t.startswith("  env-value")), "case2: TERM member line missing")
# 3. unset then delete: a key read unset inside an env_block init, then deleted: the delete is the 'X unset' member, not a blocker.
j, out, err, rows = run([
    body(1),
    (2, J + ",2:0:0", [("env_block", "[]"), ("getenv", "DEBUG=unset"), ("native.unsetenv", "DEBUG;was=unset"), ("native.unsetenv", "OTHER;was=unset")]),
    body(3)])
need(j is not None, "case3: cut.py failed " + err)
if j:
    keys = [tuple(L["key"]) for L in j["ledger"]]
    need(("unsetenv", "DEBUG") not in keys, "case3: the delete of a key read unset is a blocker")
    need(("unsetenv", "OTHER") in keys, "case3: a delete of a key never read unset must stay a blocker")
    need(any(k[0] == "env_block" for k in keys), "case3: env_block missing")
# 4. realizable boundary: init 3 runs inside the open body of init 2 (a CommonJS require); the uid read in 3 cuts before 2.
# nested frames are written by hand: enter 2, enter 3 (open 2), uid in 3, exit 3, exit 2
seq = [0]


def nested():
    rr = []

    def a(init, st, op, v=""):
        seq[0] += 1
        rr.append(row(seq[0], 100 * seq[0], init, st, op, v))
    a(0, "", "census_start"); a(0, "", "boundary")
    a(1, J + ",1:0:0", "init_enter", "x"); a(1, J + ",1:0:0", "init_exit", "threw=false")
    a(2, J + ",2:0:0", "init_enter", "x")
    a(3, J + ",2:0:0,3:0:0", "init_enter", "x")
    a(3, J + ",2:0:0,3:0:0", "ids.uid", "()")
    a(3, J + ",2:0:0,3:0:0", "init_exit", "threw=false")
    a(2, J + ",2:0:0", "init_exit", "threw=false")
    a(4, J + ",4:0:0", "init_enter", "x"); a(4, J + ",4:0:0", "init_exit", "threw=false")
    return rr


d = tempfile.mkdtemp()
rr = nested()
with open(d + "/tx-report.inputs.tsv", "w") as f:
    f.write("seq\tir\tinit\tstack_parts\top\tvalue_hex\tpcs\tallocated_bytes\n")
    for r in rr:
        f.write("\t".join(r) + "\n")
with open(d + "/tx-report", "w") as f:
    f.write("#id\taddr\n" + "".join("%d\t%x\n" % (i, 0x1000 + i) for i in range(1, 5)))
env = dict(os.environ, SNAP_CUT_DIR=d + "/", SNAP_CUT_PFX="t", SNAP_CUT_OUT=d + "/")
p = subprocess.run([sys.executable, CUT, "x", "model"], env=env, capture_output=True, text=True)
need(p.returncode == 0, "case4: cut.py failed " + p.stderr[-300:])
if p.returncode == 0:
    j = json.load(open(d + "/cutk3-x-model.json"))
    L = j["ledger"][0]
    need(L["blocking_init"] == 3 and L["outer_frame"] == 2 and L["cut_before_init"] == 2, "case4: ledger %s" % {k: L[k] for k in ("blocking_init", "outer_frame", "cut_before_init")})
    need(j["product"]["cut_before_init"] == 2, "case4: product before init %s, want 2" % j["product"]["cut_before_init"])
    need("blocking init 3, outermost open frame 2" in p.stdout, "case4: the output does not print both inits")
print("check_cut: %s" % ("FAIL " + "; ".join(fails) if fails else "ok (4 cases)"))
sys.exit(1 if fails else 0)
