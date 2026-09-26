#!/usr/bin/env python3
"""Follow-up diagnostics after bench_mini.py (same lock, same binaries):
1. backend server under concurrency: does it survive? (exit code + last output)
2. fetch client c=1 reliability: 10 runs per arm, failures captured
3. peak thread count DURING load for server / fetch client (ps -M sampling)
Writes <root>/results/mini-diag.json."""
import json, os, subprocess, sys, time, threading, signal
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bench_mini as bm
ROOT = bm.ROOT; OUT = os.path.join(bm.RES, "mini-diag.json"); D = {"backend": [], "fetch": [], "threads": []}
def peak_threads(pid, stop, box):
    while not stop.is_set():
        t = bm.threads(pid); box[0] = max(box[0], t); time.sleep(0.1)
def main():
    bm.acquire()
    try:
        for rnd in range(3):
            for a in bm.ARMS:
                for c in (8, 16, 32, 64):
                    p = subprocess.Popen([os.path.join(ROOT, a, "backend"), "serve", "18083"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
                    p.stdout.readline()
                    r = bm.oha(18083, c, "5s")
                    time.sleep(0.5); rc = p.poll()
                    if rc is None: bm.stop(p); tail = ""
                    else: tail = p.stdout.read()[-600:]
                    D["backend"].append({"round": rnd, "arm": a, "conc": c, "alive_after": rc is None, "exit": rc, "ok": r["ok"], "total": r["total"], "errors": r["errors"], "tail": tail})
                    bm.log(f"backend {a} c={c}: alive={rc is None} exit={rc} ok={r['ok']}/{r['total']} {tail[-160:]!r}")
        srv = subprocess.Popen([bm.NODE, "-e", "require('http').createServer((q,s)=>s.end('hello world')).listen(18090,'127.0.0.1',()=>console.log('listening'))"], stdout=subprocess.PIPE, text=True)
        srv.stdout.readline()
        for rnd in range(10):
            for a in bm.ARMS:
                for c in (1, 16):
                    r = subprocess.run([os.path.join(ROOT, a, "bench_fetch_client"), "http://127.0.0.1:18090/", "3000", str(c)], capture_output=True, text=True, timeout=120)
                    okline = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else ""
                    ok = r.returncode == 0 and okline.startswith("{")
                    D["fetch"].append({"round": rnd, "arm": a, "conc": c, "rc": r.returncode, "ok": ok, "stdout": r.stdout[-300:], "stderr": r.stderr[-600:]})
                    if not ok: bm.log(f"fetch {a} c={c} FAIL rc={r.returncode} out={r.stdout[-200:]!r} err={r.stderr[-300:]!r}")
        # peak threads during load
        for a in bm.ARMS:
            for kind, cmd, port in (("http_server", ["bench_http_server", "18084"], 18084), ("backend", ["backend", "serve", "18085"], 18085)):
                p = subprocess.Popen([os.path.join(ROOT, a, cmd[0])] + cmd[1:], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True); p.stdout.readline()
                box = [0]; st = threading.Event(); th = threading.Thread(target=peak_threads, args=(p.pid, st, box)); th.start()
                idle = bm.threads(p.pid); bm.oha(port, 4, "3s"); st.set(); th.join()
                D["threads"].append({"arm": a, "kind": kind, "idle": idle, "peak_under_load": box[0]}); bm.stop(p)
            for c in (1, 16):
                p = subprocess.Popen([os.path.join(ROOT, a, "bench_fetch_client"), "http://127.0.0.1:18090/", "20000", str(c)], stdout=subprocess.PIPE, text=True)
                box = [0]; st = threading.Event(); th = threading.Thread(target=peak_threads, args=(p.pid, st, box)); th.start()
                p.wait(); st.set(); th.join()
                D["threads"].append({"arm": a, "kind": f"fetch_client_c{c}", "peak_under_load": box[0]})
            for probe in ("crypto", "zlib", "worker", "timers", "net"):
                p = subprocess.Popen([os.path.join(ROOT, a, probe)], cwd=os.path.join(ROOT, "probes"), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                box = [0]; st = threading.Event(); th = threading.Thread(target=peak_threads, args=(p.pid, st, box)); th.start()
                try: p.wait(30)
                except subprocess.TimeoutExpired: p.kill()
                st.set(); th.join()
                D["threads"].append({"arm": a, "kind": probe, "peak_under_load": box[0]})
            bm.log("threads", a, [x for x in D["threads"] if x["arm"] == a])
        bm.stop(srv)
    finally:
        json.dump(D, open(OUT, "w"), indent=1); bm.release()
    bm.log("DIAG DONE")
main()
