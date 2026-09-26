#!/usr/bin/env python3
"""Package-performance harness (Phase 1): real-source npm packages on
Perry vs Node vs Bun.

The workloads live in benchmarks/packages/<pkg>/<name>.ts and are registered
in benchmarks/packages/manifest.json. Each prints only deterministic text
(resolved version + checksum), never a timing. This script:

  compile   Compile every workload with Perry (default flags: plain
            `perry compile <file> -o <bin>`, i.e. auto-optimize ON), record
            compile time, binary size, and a LIVENESS verdict: the package's
            own modules must appear in the compile's module list and the
            binary must carry none of the removed perry-ext-* binding's
            exported symbols. Liveness failure is a FAIL, never a warning.

  run       For each workload and arm (node / bun / perry):
            1. CORRECTNESS: stdout must equal Node's byte-for-byte (every
               measured run is checked, not only the first); otherwise the
               arm is MISMATCH and excluded from timing.
            2. INSTRUCTIONS (--modes instr; Linux, `perf stat -e
               instructions:u`): at n1 and n2, median of --instr-reps;
               per-iteration = (I(n2) - I(n1)) / (n2 - n1). Startup, module
               init and warm-up cancel.
            3. WALL (--modes wall): median of --wall-reps (>= 7) at n1 and n2,
               interleaved across arms, same two-N per-iteration formula.
            4. COLD (--modes cold): median wall of `<arm> <wl> 0 0`.
            5. RSS (--modes rss): peak RSS of one n2 run under /usr/bin/time.
            The host 1-minute load average is recorded with every sample;
            a workload whose max exceeds --load-threshold is flagged.

  report    Merge compile + run JSON files into a Markdown report ranking
            workloads/packages by Perry/Node ratio.

  profile   (Linux) perf-record each Perry binary at n1, keep the top-5
            symbols; feeds the report's first-pass attribution column.
            Use binaries compiled with PERRY_KEEP_SYMBOLS=1.

  lock      acquire|release|status the host measurement mutex by hand.

Measurement mutex: `run` takes a host-wide mkdir lock (default
/tmp/perry-bench-lock.d, override with PERRY_BENCH_LOCK) holding an owner
file, released on exit/SIGINT/SIGTERM. A lock whose owner PID is dead on
this host is broken automatically; otherwise `run` waits (--lock-timeout).

I/O workloads need servers. `run --servers auto` starts each required server
from the binaries given by flags/env with its OWN data dir under
--server-root and its own port, and stops it by PID on exit. A server that
cannot be started makes its workloads SKIP (reason recorded), not FAIL.

Everything runs with cwd=benchmarks/packages and TZ=UTC.

Build-id pitfall: auto-optimize rebuilds the runtime archives stamped with
the checkout's HEAD. If HEAD moved after `perry` was built (e.g. you
committed harness edits), every compile fails with "runtime library does not
match this Perry compiler". Export PERRY_BUILD_COMMIT=<full sha perry was
built at> for `compile`, and delete stale target/perry-auto-* dirs.

Typical session (what produced benchmarks/packages/REPORT.md):
  cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
  (cd benchmarks/packages && npm ci --ignore-scripts)
  python3 scripts/package_bench.py compile --perry-bin-dir OUT/linux
  python3 scripts/package_bench.py run --perry-bin-dir OUT/linux --modes instr \
      --out OUT/instr.json --server-root /srv/... --pg-bin-dir ... --mysqld ... --mongod ...
  # macOS host: binaries compiled on a Mac, then
  python3 scripts/package_bench.py run --perry-bin-dir OUT/mac --modes wall,cold,rss --out OUT/wall.json
  python3 scripts/package_bench.py report --instr OUT/instr.json --wall OUT/wall.json \
      --compile OUT/linux/compile.json
"""

from __future__ import annotations

import argparse
import atexit
import json
import os
import platform
import re
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PKG_DIR = REPO / "benchmarks" / "packages"
MANIFEST = PKG_DIR / "manifest.json"
IS_LINUX = platform.system() == "Linux"
IS_MAC = platform.system() == "Darwin"


def log(msg: str) -> None:
    print(f"[package_bench {time.strftime('%H:%M:%S')}] {msg}", file=sys.stderr, flush=True)


def load_manifest() -> dict:
    return json.loads(MANIFEST.read_text())


def select_workloads(manifest: dict, filters: list[str] | None) -> list[dict]:
    wls = manifest["workloads"]
    if not filters:
        return wls
    out = []
    for w in wls:
        if w["id"].startswith("control/") or any(f in w["id"] for f in filters):
            out.append(w)
    return out


def pkg_of(wid: str) -> str:
    return wid.split("/", 1)[0]


def bin_name(wid: str) -> str:
    return wid.replace("/", "__").replace(".", "_")


# ---------------------------------------------------------------- toolchain

def pinned_node_version() -> str:
    return (REPO / ".node-version").read_text().strip().lstrip("v")


def pinned_bun_version() -> str:
    cfg = json.loads((REPO / "benchmarks" / "public-baseline-config.json").read_text())
    for k in ("toolchains", "versions"):
        if isinstance(cfg.get(k), dict) and "bun" in cfg[k]:
            return str(cfg[k]["bun"])
    m = re.search(r'"bun"\s*:\s*"([^"]+)"', json.dumps(cfg))
    return m.group(1) if m else "1.3.14"


def tool_version(exe: str, flag: str = "--version") -> str:
    try:
        return subprocess.run([exe, flag], capture_output=True, text=True, timeout=30).stdout.strip().lstrip("v")
    except Exception as e:  # noqa: BLE001
        return f"unavailable ({e})"


def default_node() -> str:
    if os.environ.get("PKG_BENCH_NODE"):
        return os.environ["PKG_BENCH_NODE"]
    v = pinned_node_version()
    for c in (f"/opt/node-v{v}-linux-x64/bin/node", str(Path.home() / f"node-v{v}-darwin-arm64/bin/node")):
        if Path(c).exists():
            return c
    return shutil.which("node") or "node"


def default_bun() -> str:
    if os.environ.get("PKG_BENCH_BUN"):
        return os.environ["PKG_BENCH_BUN"]
    c = Path.home() / ".bun" / "bin" / "bun"
    return str(c) if c.exists() else (shutil.which("bun") or "bun")


def check_toolchain(args) -> dict:
    tc = {"node": args.node, "node_version": tool_version(args.node), "bun": args.bun,
          "bun_version": tool_version(args.bun)}
    want_node, want_bun = pinned_node_version(), pinned_bun_version()
    problems = []
    if "node" in args.arms and tc["node_version"] != want_node:
        problems.append(f"node is {tc['node_version']}, pinned {want_node} (.node-version)")
    if "bun" in args.arms and tc["bun_version"] != want_bun:
        problems.append(f"bun is {tc['bun_version']}, pinned {want_bun} (public-baseline-config.json)")
    if problems and not args.allow_version_mismatch:
        sys.exit("refusing to run: " + "; ".join(problems) + " (pass --allow-version-mismatch to override)")
    tc["version_problems"] = problems
    return tc


# ---------------------------------------------------------------- lock

def lock_dir() -> Path:
    return Path(os.environ.get("PERRY_BENCH_LOCK", "/tmp/perry-bench-lock.d"))


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


_LOCK_HELD = False


def lock_release() -> None:
    global _LOCK_HELD
    if not _LOCK_HELD:
        return
    d = lock_dir()
    try:
        (d / "owner.json").unlink(missing_ok=True)
        d.rmdir()
    except OSError as e:
        log(f"lock release problem: {e}")
    _LOCK_HELD = False


def lock_acquire(owner: str, timeout_s: float) -> None:
    global _LOCK_HELD
    d = lock_dir()
    deadline = time.time() + timeout_s
    last_note = 0.0
    while True:
        try:
            d.mkdir()
        except FileExistsError:
            info = {}
            try:
                info = json.loads((d / "owner.json").read_text())
            except Exception:  # noqa: BLE001
                pass
            pid, host = info.get("pid"), info.get("host")
            if pid and host == socket.gethostname() and not _pid_alive(int(pid)):
                log(f"breaking stale lock held by dead pid {pid} ({info.get('owner')})")
                (d / "owner.json").unlink(missing_ok=True)
                try:
                    d.rmdir()
                except OSError:
                    pass
                continue
            if not info and time.time() - d.stat().st_mtime > 60:
                log("breaking ownerless lock older than 60 s")
                try:
                    d.rmdir()
                except OSError:
                    pass
                continue
            if time.time() > deadline:
                sys.exit(f"measurement lock {d} still held by {info} after {timeout_s:.0f}s")
            if time.time() - last_note > 60:
                log(f"waiting for measurement lock {d}: held by {info}")
                last_note = time.time()
            time.sleep(5)
            continue
        (d / "owner.json").write_text(json.dumps({
            "owner": owner, "pid": os.getpid(), "host": socket.gethostname(),
            "started": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "cmd": " ".join(sys.argv)}))
        _LOCK_HELD = True
        atexit.register(lock_release)
        return


# ---------------------------------------------------------------- servers

_SERVERS: list[tuple[str, subprocess.Popen]] = []


def stop_servers() -> None:
    while _SERVERS:
        name, p = _SERVERS.pop()
        if p.poll() is None:
            log(f"stopping {name} (pid {p.pid})")
            p.terminate()
            try:
                p.wait(timeout=60)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()


def _on_signal(signum, _frame):
    stop_servers()
    lock_release()
    sys.exit(128 + signum)


def wait_port(port: int, timeout: float = 60.0) -> bool:
    end = time.time() + timeout
    while time.time() < end:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=1):
                return True
        except OSError:
            time.sleep(0.3)
    return False


def _as_user(user: str | None) -> list[str]:
    if user and os.geteuid() == 0:
        return ["runuser", "-u", user, "--"]
    return []


def start_http(args, env: dict) -> str | None:
    p = subprocess.Popen([args.node, str(PKG_DIR / "_servers" / "http_server.mjs")], cwd=PKG_DIR,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    line = p.stdout.readline().strip()
    if not line.startswith("listening "):
        p.kill()
        return f"http server did not start: {line!r}"
    _SERVERS.append(("http", p))
    env["PKG_BENCH_HTTP_PORT"] = line.split()[1]
    return None


def start_pg(args, env: dict) -> str | None:
    bindir = args.pg_bin_dir
    if not bindir or not (Path(bindir) / "postgres").exists():
        return "no postgres binaries (--pg-bin-dir / PKG_BENCH_PG_BIN_DIR)"
    root = Path(args.server_root) / "pg"
    user = args.pg_user if os.geteuid() == 0 else None
    # An extracted (not installed) PostgreSQL .deb keeps libpq beside its
    # bin dir: <root>/usr/lib/postgresql/16/bin -> <root>/usr/lib/<triple>/.
    penv = dict(os.environ)
    for lib in Path(bindir).resolve().parents[2].glob("*-linux-gnu"):
        if (lib / "libpq.so.5").exists():
            penv["LD_LIBRARY_PATH"] = str(lib) + (":" + penv["LD_LIBRARY_PATH"] if penv.get("LD_LIBRARY_PATH") else "")
    if not (root / "PG_VERSION").exists():
        root.mkdir(parents=True, exist_ok=True)
        if user:
            subprocess.run(["chown", "-R", user, str(root)], check=True)
        r = subprocess.run(_as_user(user) + [f"{bindir}/initdb", "-D", str(root), "-U", "bench", "--auth=trust"],
                           capture_output=True, text=True, env=penv)
        if r.returncode:
            return "initdb failed: " + r.stderr[-400:]
    port = args.pg_port
    p = subprocess.Popen(_as_user(user) + [f"{bindir}/postgres", "-D", str(root), "-p", str(port), "-k", str(root),
                                           "-c", "listen_addresses=127.0.0.1", "-c", "fsync=off"],
                         stdout=subprocess.DEVNULL, stderr=open(Path(args.server_root) / "pg.log", "a"), env=penv)
    _SERVERS.append(("postgres", p))
    if not wait_port(port):
        return "postgres did not open its port"
    time.sleep(1)
    subprocess.run([f"{bindir}/createdb", "-h", "127.0.0.1", "-p", str(port), "-U", "bench", "bench"],
                   capture_output=True, text=True, env=penv)  # exists after first run
    env["PKG_BENCH_PG_PORT"] = str(port)
    return None


def start_mysql(args, env: dict) -> str | None:
    mysqld = args.mysqld
    if not mysqld or not Path(mysqld).exists():
        return "no mysqld (--mysqld / PKG_BENCH_MYSQLD)"
    root = Path(args.server_root)
    data = root / "mysql"
    user = "mysql" if os.geteuid() == 0 else None
    common = ["--no-defaults"] + ([f"--user={user}"] if user else [])
    if not data.exists():
        data.parent.mkdir(parents=True, exist_ok=True)
        r = subprocess.run([mysqld] + common + ["--initialize-insecure", f"--datadir={data}"],
                           capture_output=True, text=True)
        if r.returncode:
            return "mysqld --initialize-insecure failed: " + r.stderr[-400:]
    sock = root / "mysqld.sock"
    port = args.mysql_port
    p = subprocess.Popen([mysqld] + common + [f"--datadir={data}", f"--port={port}", "--bind-address=127.0.0.1",
                                              f"--socket={sock}", "--mysqlx=OFF", f"--pid-file={root / 'mysqld.pid'}",
                                              f"--log-error={root / 'mysqld.err'}", "--skip-log-bin",
                                              "--innodb-flush-log-at-trx-commit=0"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _SERVERS.append(("mysqld", p))
    if not wait_port(port, 120):
        return "mysqld did not open its port"
    client = shutil.which("mysql")
    if not client:
        return "no mysql client to create the bench user"
    sql = ("CREATE USER IF NOT EXISTS 'bench'@'%' IDENTIFIED WITH mysql_native_password BY 'bench';"
           "CREATE DATABASE IF NOT EXISTS bench; GRANT ALL ON bench.* TO 'bench'@'%'; FLUSH PRIVILEGES;")
    r = subprocess.run([client, "--no-defaults", "-uroot", f"--socket={sock}", "-e", sql], capture_output=True, text=True)
    if r.returncode:
        return "mysql user setup failed: " + r.stderr[-400:]
    env["PKG_BENCH_MYSQL_PORT"] = str(port)
    return None


def start_mongo(args, env: dict) -> str | None:
    mongod = args.mongod
    if not mongod or not Path(mongod).exists():
        return "no mongod (--mongod / PKG_BENCH_MONGOD)"
    data = Path(args.server_root) / "mongo"
    data.mkdir(parents=True, exist_ok=True)
    port = args.mongo_port
    p = subprocess.Popen([mongod, "--port", str(port), "--dbpath", str(data), "--bind_ip", "127.0.0.1",
                          "--logpath", str(Path(args.server_root) / "mongod.log")],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _SERVERS.append(("mongod", p))
    if not wait_port(port, 120):
        return "mongod did not open its port"
    env["PKG_BENCH_MONGO_PORT"] = str(port)
    return None


def start_redis(args, env: dict) -> str | None:
    rs = args.redis_server
    if not rs or not Path(rs).exists():
        return "no redis-server (--redis-server / PKG_BENCH_REDIS_SERVER)"
    data = Path(args.server_root) / "redis"
    data.mkdir(parents=True, exist_ok=True)
    port = args.redis_port
    p = subprocess.Popen([rs, "--port", str(port), "--bind", "127.0.0.1", "--save", "", "--appendonly", "no",
                          "--dir", str(data)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _SERVERS.append(("redis-server", p))
    if not wait_port(port):
        return "redis-server did not open its port"
    env["PKG_BENCH_REDIS_PORT"] = str(port)
    return None


STARTERS = {"http": start_http, "pg": start_pg, "mysql": start_mysql, "mongo": start_mongo, "redis": start_redis}


# ---------------------------------------------------------------- execution

def arm_cmd(args, arm: str, wid: str, n: int, warm: int) -> list[str]:
    src = str(PKG_DIR / f"{wid}.ts")
    if arm == "node":
        return [args.node, "--no-warnings", src, str(n), str(warm)]
    if arm == "bun":
        return [args.bun, src, str(n), str(warm)]
    if arm == "perry":
        return [str(Path(args.perry_bin_dir) / bin_name(wid)), str(n), str(warm)]
    raise ValueError(arm)


def run_env(extra: dict) -> dict:
    e = dict(os.environ)
    e["TZ"] = "UTC"
    e.pop("NODE_OPTIONS", None)
    e.update(extra)
    return e


def run_once(cmd: list[str], env: dict, timeout: float, prefix: list[str] | None = None):
    """Returns (rc, stdout, stderr, wall_s, load1)."""
    load1 = os.getloadavg()[0]
    t0 = time.perf_counter()
    try:
        p = subprocess.run((prefix or []) + cmd, cwd=PKG_DIR, env=env, capture_output=True, text=True,
                           timeout=timeout)
        rc, out, err = p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired as e:
        rc, out, err = -999, (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), \
            f"TIMEOUT after {timeout}s"
    return rc, out, err, time.perf_counter() - t0, load1


def perf_instructions(cmd, env, timeout):
    with tempfile.NamedTemporaryFile(suffix=".perf", delete=False) as f:
        out_path = f.name
    try:
        rc, out, err, wall, load1 = run_once(cmd, env, timeout,
                                             prefix=["perf", "stat", "-x", ",", "-e", "instructions:u", "-o", out_path, "--"])
        instr = None
        for line in Path(out_path).read_text().splitlines():
            parts = line.split(",")
            if len(parts) > 2 and parts[2].startswith("instructions"):
                try:
                    instr = int(parts[0])
                except ValueError:
                    instr = None
        return rc, out, err, instr, load1
    finally:
        os.unlink(out_path)


def peak_rss_kb(cmd, env, timeout):
    if IS_LINUX:
        rc, out, err, _w, _l = run_once(cmd, env, timeout, prefix=["/usr/bin/time", "-v"])
        m = re.search(r"Maximum resident set size \(kbytes\): (\d+)", err)
        return rc, out, (int(m.group(1)) if m else None)
    rc, out, err, _w, _l = run_once(cmd, env, timeout, prefix=["/usr/bin/time", "-l"])
    m = re.search(r"(\d+)\s+maximum resident set size", err)
    return rc, out, (int(m.group(1)) // 1024 if m else None)


def first_diff(a: str, b: str) -> str:
    al, bl = a.splitlines(), b.splitlines()
    for i in range(max(len(al), len(bl))):
        x = al[i] if i < len(al) else "<missing>"
        y = bl[i] if i < len(bl) else "<missing>"
        if x != y:
            return f"line {i + 1}: node={x[:160]!r} arm={y[:160]!r}"
    return "identical"


def tail(s: str, n: int = 600) -> str:
    s = s.strip()
    return s[-n:]


def cmd_run(args) -> None:
    manifest = load_manifest()
    wls = select_workloads(manifest, args.filter)
    args.arms = [a.strip() for a in args.arms.split(",") if a.strip()]
    modes = set(m.strip() for m in args.modes.split(","))
    tc = check_toolchain(args)
    if "instr" in modes and not IS_LINUX:
        sys.exit("--modes instr needs Linux perf")
    compile_info = {}
    if "perry" in args.arms:
        cj = Path(args.perry_bin_dir) / "compile.json"
        if cj.exists():
            compile_info = json.loads(cj.read_text()).get("workloads", {})
    signal.signal(signal.SIGINT, _on_signal)
    signal.signal(signal.SIGTERM, _on_signal)
    atexit.register(stop_servers)
    lock_acquire(args.owner, args.lock_timeout)
    ncpu = os.cpu_count() or 1
    threshold = args.load_threshold if args.load_threshold is not None else ncpu * 0.5

    env_extra: dict = {}
    server_status: dict = {}
    needed = sorted({s for w in wls for s in w.get("servers", [])})
    if needed and args.servers == "auto":
        Path(args.server_root).mkdir(parents=True, exist_ok=True)
        for s in needed:
            log(f"starting server {s}")
            server_status[s] = STARTERS[s](args, env_extra) or "ok"
            log(f"server {s}: {server_status[s]}")
    elif needed:
        for s in needed:
            server_status[s] = "ok" if args.servers == "external" else "not started (--servers none)"
    env = run_env(env_extra)

    results = {
        "schema": 1, "host": socket.gethostname(), "platform": platform.platform(), "machine": platform.machine(),
        "ncpu": ncpu, "started": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "toolchain": tc,
        "perry_bin_dir": args.perry_bin_dir, "perry_commit": args.perry_commit, "modes": sorted(modes),
        "load_threshold": threshold, "servers": server_status, "workloads": {},
    }
    out_path = Path(args.out)

    def save():
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(results, indent=1, sort_keys=True))

    for w in wls:
        wid, n1, n2, warm = w["id"], w["n1"], w["n2"], w["warm"]
        if args.scale != 1.0:
            n1, n2 = max(1, int(n1 * args.scale)), max(2, int(n2 * args.scale))
        entry = {"n1": n1, "n2": n2, "warm": warm, "arms": {}}
        results["workloads"][wid] = entry
        missing = [s for s in w.get("servers", []) if server_status.get(s) != "ok"]
        if missing:
            for arm in args.arms:
                entry["arms"][arm] = {"status": "SKIP",
                                      "reason": "; ".join(f"{s}: {server_status.get(s)}" for s in missing)}
            log(f"{wid}: SKIP ({missing})")
            save()
            continue
        log(f"{wid}: correctness at n={n1}")
        ref = None
        loads: dict = {a: [] for a in args.arms}
        for arm in (["node"] + [a for a in args.arms if a != "node"]):
            if arm not in args.arms and arm != "node":
                continue
            a = entry["arms"].setdefault(arm, {})
            if arm == "perry":
                ci = compile_info.get(wid)
                if ci is None or ci.get("status") != "OK":
                    a.update(status="FAIL", reason="perry " + (ci or {}).get("reason", "binary not compiled"))
                    continue
            rc, out, err, wall, load1 = run_once(arm_cmd(args, arm, wid, n1, warm), env, args.timeout)
            a["correctness_output"] = out.strip()[:400]
            if rc != 0 or out.startswith("ERROR") or "\nERROR" in out:
                a.update(status="FAIL", reason=f"exit {rc}: " + tail(out + "\n" + err, 400))
            elif arm == "node":
                ref = out
                a["status"] = "OK"
            elif ref is None:
                a.update(status="FAIL", reason="no node reference output")
            elif out != ref:
                a.update(status="MISMATCH", reason=first_diff(ref, out))
            else:
                a["status"] = "OK"
        if "node" not in args.arms:
            entry["arms"].pop("node", None)
        ok_arms = [arm for arm in args.arms if entry["arms"].get(arm, {}).get("status") == "OK"]
        refs: dict = {}

        def check_out(arm, n, out):
            key = (n, warm)
            if arm == "node" or "node" not in ok_arms:
                refs.setdefault(key, out)
                return
            r = refs.get(key)
            if r is not None and r != out and entry["arms"][arm]["status"] == "OK":
                entry["arms"][arm].update(status="MISMATCH", reason=f"at n={n}: " + first_diff(r, out))

        order = (["node"] if "node" in ok_arms else []) + [a for a in ok_arms if a != "node"]
        if "instr" in modes:
            log(f"{wid}: instructions ({order})")
            for arm in order:
                vals: dict = {}
                for n in (n1, n2):
                    samples = []
                    for _ in range(args.instr_reps):
                        rc, out, err, instr, load1 = perf_instructions(arm_cmd(args, arm, wid, n, warm), env, args.timeout)
                        loads[arm].append(load1)
                        if rc != 0 or instr is None:
                            entry["arms"][arm].update(status="FAIL", reason=f"perf run exit {rc}: " + tail(err, 300))
                            break
                        check_out(arm, n, out)
                        samples.append(instr)
                    vals[n] = samples
                if entry["arms"][arm]["status"] != "OK" or len(vals.get(n2, [])) < args.instr_reps:
                    continue
                i1, i2 = statistics.median(vals[n1]), statistics.median(vals[n2])
                entry["arms"][arm]["instr"] = {"n1": vals[n1], "n2": vals[n2],
                                               "per_iter": (i2 - i1) / (n2 - n1)}
        if "wall" in modes:
            log(f"{wid}: wall x{args.wall_reps} ({order})")
            samples = {arm: {n1: [], n2: []} for arm in order}
            for _rep in range(args.wall_reps):
                for arm in order:
                    if entry["arms"][arm]["status"] != "OK":
                        continue
                    for n in (n1, n2):
                        rc, out, err, wall, load1 = run_once(arm_cmd(args, arm, wid, n, warm), env, args.timeout)
                        loads[arm].append(load1)
                        if rc != 0:
                            entry["arms"][arm].update(status="FAIL", reason=f"wall run exit {rc}: " + tail(err, 300))
                            break
                        check_out(arm, n, out)
                        samples[arm][n].append(wall)
            for arm in order:
                s = samples[arm]
                if entry["arms"][arm]["status"] != "OK" or len(s[n2]) < args.wall_reps:
                    continue
                t1, t2 = statistics.median(s[n1]), statistics.median(s[n2])
                entry["arms"][arm]["wall"] = {"n1_s": s[n1], "n2_s": s[n2], "per_iter_s": (t2 - t1) / (n2 - n1),
                                              "n2_median_s": t2}
        if "cold" in modes:
            for arm in order:
                if entry["arms"][arm]["status"] != "OK":
                    continue
                ts = []
                for _ in range(args.wall_reps):
                    rc, out, err, wall, load1 = run_once(arm_cmd(args, arm, wid, 0, 0), env, args.timeout)
                    loads[arm].append(load1)
                    if rc == 0:
                        ts.append(wall)
                if ts:
                    entry["arms"][arm]["cold_s"] = statistics.median(ts)
        if "rss" in modes:
            for arm in order:
                if entry["arms"][arm]["status"] != "OK":
                    continue
                rc, out, kb = peak_rss_kb(arm_cmd(args, arm, wid, n2, warm), env, args.timeout)
                if rc == 0:
                    entry["arms"][arm]["peak_rss_kb"] = kb
        for arm in order:
            if loads.get(arm):
                mx = max(loads[arm])
                entry["arms"][arm]["load_max"] = round(mx, 2)
                entry["arms"][arm]["load_mean"] = round(statistics.mean(loads[arm]), 2)
                entry["arms"][arm]["load_flagged"] = mx > threshold
        if "perry" in entry["arms"] and wid in compile_info:
            ci = compile_info[wid]
            entry["arms"]["perry"].update(size_bytes=ci.get("size_bytes"), compile_s=ci.get("compile_s"))
        log(f"{wid}: " + ", ".join(f"{a}={entry['arms'][a].get('status')}" for a in entry["arms"]))
        save()
    results["finished"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    save()
    stop_servers()
    lock_release()
    log(f"wrote {out_path}")


# ---------------------------------------------------------------- compile

AUDIT_JSON = PKG_DIR / "node_modules" / ".cache" / "perry" / "audit.json"


def compiled_modules(log_text: str) -> dict:
    """Module census of the compile that just ran, from Perry's behavioral
    SBOM (node_modules/.cache/perry/audit.json, rewritten by every compile:
    one entry per NATIVELY compiled source module, with its package) plus the
    "Found N module(s): X native, Y JavaScript" line (Y > 0 would mean a JS
    runtime fallback)."""
    by_pkg: dict = {}
    try:
        for m in json.loads(AUDIT_JSON.read_text()).get("modules", []):
            k = m.get("package") or "<app>"
            by_pkg[k] = by_pkg.get(k, 0) + 1
    except (OSError, ValueError):
        pass
    found = re.search(r"Found (\d+) module\(s\): (\d+) native, (\d+) JavaScript", log_text)
    return {"by_package": by_pkg, "native": int(found.group(2)) if found else None,
            "javascript": int(found.group(3)) if found else None}


def defined_symbols(binary: Path) -> set | None:
    """Global defined symbols, or None when the binary has no symbol table
    (Perry strips by default — compile with PERRY_KEEP_SYMBOLS=1 to make the
    binding-symbol half of the liveness check live)."""
    nm = shutil.which("nm")
    if not nm:
        return None
    p = subprocess.run([nm, "-g", "--defined-only", str(binary)] if IS_LINUX else [nm, "-gU", str(binary)],
                       capture_output=True, text=True)
    syms = set()
    for line in p.stdout.splitlines():
        parts = line.split()
        if parts:
            syms.add(parts[-1][1:] if IS_MAC and parts[-1].startswith("_") else parts[-1])
    return syms or None


def binding_symbols(syms: set, prefixes: list[str], baseline: set) -> list[str]:
    """Symbols carrying a removed binding's prefix, minus `baseline`: the
    bare-loop control binary's symbols plus the manifest's event-loop hooks
    (js_cron_timer_* are called by the generated event loop whenever stdlib is
    linked, so they say nothing about how the package was routed)."""
    return sorted(x for x in syms if any(x.startswith(pf) for pf in prefixes) and x not in baseline)


def cmd_compile(args) -> None:
    manifest = load_manifest()
    wls = select_workloads(manifest, args.filter)
    out = Path(args.perry_bin_dir)
    (out / "logs").mkdir(parents=True, exist_ok=True)
    cj = out / "compile.json"
    data = json.loads(cj.read_text()) if cj.exists() else {"workloads": {}}
    perry_version = tool_version(args.perry)
    data.update(perry=args.perry, perry_version=perry_version, perry_commit=args.perry_commit,
                host=socket.gethostname(), platform=platform.platform(),
                flags=args.perry_flags, env={k: v for k, v in os.environ.items() if k.startswith("PERRY_")})
    baseline_syms: set = set(manifest.get("event_loop_hook_symbols", {}).get("symbols", []))
    for w in wls:
        wid = w["id"]
        pkg = pkg_of(wid)
        binary = out / bin_name(wid)
        cmd = [args.perry, "compile", f"{wid}.ts", "-o", str(binary)] + args.perry_flags.split()
        log(f"compile {wid}")
        binary.unlink(missing_ok=True)
        AUDIT_JSON.unlink(missing_ok=True)
        load1 = os.getloadavg()[0]
        t0 = time.perf_counter()
        try:
            p = subprocess.run(cmd, cwd=PKG_DIR, capture_output=True, text=True, timeout=args.compile_timeout,
                               env=run_env({}))
            rc, text = p.returncode, p.stdout + "\n" + p.stderr
        except subprocess.TimeoutExpired:
            rc, text = -999, f"TIMEOUT after {args.compile_timeout}s"
        dt = time.perf_counter() - t0
        (out / "logs" / f"{bin_name(wid)}.log").write_text(" ".join(cmd) + "\n" + text)
        rec = {"compile_s": round(dt, 2), "load1": round(load1, 2), "cmd": " ".join(cmd),
               # The first compile of a new auto-optimize feature set rebuilds
               # the runtime+stdlib archives (minutes); later ones reuse them.
               "runtime_rebuilt": "auto-optimize: rebuilding" in text}
        if rc != 0 or not binary.exists():
            rec.update(status="FAIL", reason=f"compile failed (exit {rc}): " + tail(text, 500))
        else:
            rec["size_bytes"] = binary.stat().st_size
            census = compiled_modules(text)
            hits = census["by_package"].get(pkg, 0)
            allsyms = defined_symbols(binary)
            if pkg == "control":
                baseline_syms |= allsyms or set()
            if allsyms is None:
                syms = []
                symbol_check = "not run: stripped binary (set PERRY_KEEP_SYMBOLS=1)"
            else:
                syms = binding_symbols(allsyms, manifest["packages"].get(pkg, {}).get("binding_symbols", []),
                                       baseline_syms)
                symbol_check = f"ran over {len(allsyms)} symbols"
            rec["liveness"] = {"package_modules_compiled": hits, "modules_by_package": census["by_package"],
                               "native_modules": census["native"], "javascript_modules": census["javascript"],
                               "binding_symbols": syms[:20], "symbol_check": symbol_check}
            if pkg != "control" and hits == 0:
                rec.update(status="FAIL", reason=f"liveness: no module of package {pkg} was compiled natively "
                                                 f"(audit.json census: {census['by_package']})")
            elif census["javascript"]:
                rec.update(status="FAIL", reason=f"liveness: {census['javascript']} module(s) routed to a JS runtime")
            elif syms:
                rec.update(status="FAIL", reason=f"liveness: removed-binding symbols present: {syms[:5]}")
            else:
                rec["status"] = "OK"
        data["workloads"][wid] = rec
        log(f"compile {wid}: {rec['status']} {rec.get('reason', '')[:200]}")
        cj.write_text(json.dumps(data, indent=1, sort_keys=True))


# ---------------------------------------------------------------- profile

def cmd_profile(args) -> None:
    """First-pass attribution (Linux): `perf record -e cycles:u` each Perry
    binary at n1 and keep the top-N symbols. Point --perry-bin-dir at a set
    compiled with PERRY_KEEP_SYMBOLS=1 (default binaries are stripped)."""
    if not IS_LINUX:
        sys.exit("profile needs Linux perf")
    manifest = load_manifest()
    wls = [w for w in select_workloads(manifest, args.filter) if not w["id"].startswith("control/")
           or args.include_control]
    signal.signal(signal.SIGINT, _on_signal)
    signal.signal(signal.SIGTERM, _on_signal)
    atexit.register(stop_servers)
    env_extra: dict = {}
    status: dict = {}
    for srv in sorted({s for w in wls for s in w.get("servers", [])}):
        Path(args.server_root).mkdir(parents=True, exist_ok=True)
        status[srv] = STARTERS[srv](args, env_extra) or "ok"
    env = run_env(env_extra)
    out: dict = json.loads(Path(args.out).read_text()) if Path(args.out).exists() else {}
    for w in wls:
        wid = w["id"]
        if any(status.get(srv) != "ok" for srv in w.get("servers", [])):
            continue
        binary = Path(args.perry_bin_dir) / bin_name(wid)
        if not binary.exists():
            continue
        data = Path(tempfile.gettempdir()) / f"pkgbench-{os.getpid()}.perf"
        rc, _o, err, _w, _l = run_once(
            ["perf", "record", "-q", "-F", "999", "-e", "cycles:u", "-o", str(data), "--",
             str(binary), str(w["n1"]), str(w["warm"])], env, args.timeout)
        rep = subprocess.run(["perf", "report", "-i", str(data), "--stdio", "--no-children", "--sort", "symbol",
                              "-q"], capture_output=True, text=True)
        data.unlink(missing_ok=True)
        top = []
        for line in rep.stdout.splitlines():
            m = re.match(r"\s*([\d.]+)%\s+\[\.\]\s+(.+?)(\s+-\s+-)?\s*$", line)
            if m:
                top.append([float(m.group(1)), m.group(2).strip()])
            if len(top) >= args.top:
                break
        out[wid] = {"rc": rc, "top": top}
        log(f"{wid}: " + ", ".join(f"{n} {p:.0f}%" for p, n in top))
        Path(args.out).write_text(json.dumps(out, indent=1, sort_keys=True))
    stop_servers()


# ---------------------------------------------------------------- report

def fmt_ratio(x):
    if x is None:
        return "—"
    return f"{x:.3f}×" if x < 0.1 else f"{x:.2f}×"


def fmt_instr(x):
    if x is None:
        return "—"
    if abs(x) < 1e4:
        return f"{x:.0f}"
    return f"{x / 1000:.1f}k" if abs(x) < 1e6 else f"{x / 1e6:.2f}M"


def fmt_us(s):
    if s is None:
        return "—"
    return f"{s * 1e6:.3f}" if abs(s) < 1e-6 else f"{s * 1e6:.1f}"


def geomean(xs):
    xs = [x for x in xs if x and x > 0]
    if not xs:
        return None
    import math
    return math.exp(sum(math.log(x) for x in xs) / len(xs))


def instr_spread(ins):
    """(max - min) / median of the instruction samples, worst of the two Ns."""
    if not ins:
        return None
    out = 0.0
    for k in ("n1", "n2"):
        xs = ins.get(k) or []
        if len(xs) >= 2 and statistics.median(xs) > 0:
            out = max(out, (max(xs) - min(xs)) / statistics.median(xs))
    return round(out, 4)


def short_sym(name: str) -> str:
    """Trim a demangled Rust/LLVM symbol to something that fits a table cell."""
    n = re.sub(r"::h[0-9a-f]{16}$", "", name)
    n = re.sub(r"<[^<>]*>", "", n)
    n = n.replace("perry_runtime::", "").replace("perry_stdlib::", "")
    return "`" + (n if len(n) <= 48 else n[:45] + "...").replace("|", "/") + "`"


def cmd_report(args) -> None:
    manifest = load_manifest()
    instr = json.loads(Path(args.instr).read_text()) if args.instr else None
    wall = json.loads(Path(args.wall).read_text()) if args.wall else None
    comp = json.loads(Path(args.compile).read_text()) if args.compile else None
    liv = json.loads(Path(args.liveness).read_text()) if args.liveness else None
    notes = json.loads(Path(args.notes).read_text()) if args.notes and Path(args.notes).exists() else {}
    if args.profile and Path(args.profile).exists():
        for wid, pr in json.loads(Path(args.profile).read_text()).items():
            auto = ", ".join(f"{short_sym(n)} {p:.0f}%" for p, n in pr.get("top", [])[:5])
            if auto:
                notes[wid] = (notes[wid] + " — " if notes.get(wid) else "") + "top-5: " + auto
    rows = []
    merged: dict = {"schema": 1, "instr_host": instr and {k: instr[k] for k in ("host", "platform", "toolchain", "started", "finished", "load_threshold", "servers") if k in instr},
                    "wall_host": wall and {k: wall[k] for k in ("host", "platform", "toolchain", "started", "finished", "load_threshold", "servers") if k in wall},
                    "compile": comp and {k: comp[k] for k in ("perry_version", "perry_commit", "host", "flags", "platform") if k in comp},
                    "workloads": {}}
    for w in manifest["workloads"]:
        wid = w["id"]
        ie = (instr or {}).get("workloads", {}).get(wid, {})
        we = (wall or {}).get("workloads", {}).get(wid, {})
        ce = (comp or {}).get("workloads", {}).get(wid, {})
        rec = {"arms": {}}
        for arm in ("node", "bun", "perry"):
            ia, wa = ie.get("arms", {}).get(arm, {}), we.get("arms", {}).get(arm, {})
            if not ia and not wa:
                continue
            r = {
                "status_instr_host": ia.get("status"), "reason_instr_host": ia.get("reason"),
                "status_wall_host": wa.get("status"), "reason_wall_host": wa.get("reason"),
                "instr_per_iter": (ia.get("instr") or {}).get("per_iter"),
                "wall_per_iter_s": (wa.get("wall") or {}).get("per_iter_s"),
                "cold_s": wa.get("cold_s"), "peak_rss_kb": wa.get("peak_rss_kb") or ia.get("peak_rss_kb"),
                "peak_rss_kb_instr_host": ia.get("peak_rss_kb"),
                "load_flagged_instr_host": ia.get("load_flagged"), "load_flagged_wall_host": wa.get("load_flagged"),
                "load_max_wall_host": wa.get("load_max"), "load_max_instr_host": ia.get("load_max"),
                "instr_spread": instr_spread(ia.get("instr")),
            }
            if arm == "perry":
                r.update(size_bytes=ce.get("size_bytes"), compile_s=ce.get("compile_s"),
                         runtime_rebuilt=ce.get("runtime_rebuilt"),
                         compile_status=ce.get("status"), compile_reason=ce.get("reason"))
            rec["arms"][arm] = r
        pn = rec["arms"].get("node", {})
        pp = rec["arms"].get("perry", {})
        pb = rec["arms"].get("bun", {})

        def ratio(key, a, b):
            x, y = a.get(key), b.get(key)
            return (x / y) if (x is not None and y and y > 0 and x > 0) else None
        rec["perry_node_instr"] = ratio("instr_per_iter", pp, pn)
        rec["perry_node_wall"] = ratio("wall_per_iter_s", pp, pn)
        rec["bun_node_instr"] = ratio("instr_per_iter", pb, pn)
        rec["bun_node_wall"] = ratio("wall_per_iter_s", pb, pn)
        rec["note"] = notes.get(wid)
        merged["workloads"][wid] = rec
        rows.append((wid, rec))
    Path(args.json_out).write_text(json.dumps(merged, indent=1, sort_keys=True))

    def perry_problem(rec):
        pp = rec["arms"].get("perry", {})
        out = []
        for host in ("instr_host", "wall_host"):
            st = pp.get(f"status_{host}")
            if st and st != "OK":
                out.append((host, st, pp.get(f"reason_{host}")))
        if pp.get("compile_status") and pp.get("compile_status") != "OK":
            out.append(("compile", "FAIL", pp.get("compile_reason")))
        return out

    L = []
    L.append("# Package performance report — Phase 1\n")
    L.append("Real-source npm packages (their perry-ext-* native bindings removed) on **Perry vs Node vs Bun**. "
             "Generated by `scripts/package_bench.py report`; raw data in the JSON files next to this report. "
             "Ratios are **Perry / Node** (and Bun / Node): **> 1 means slower than Node**.\n")
    L.append("## Setup\n")
    if comp:
        L.append(f"- Perry: `{comp.get('perry_version')}` at commit `{comp.get('perry_commit')}`, compiled with "
                 f"`perry compile <wl>.ts -o <bin>{(' ' + comp.get('flags')) if comp.get('flags') else ''}` "
                 f"(auto-optimize ON, the default) on `{comp.get('host')}`.")
    for label, d in (("Instructions", instr), ("Wall-clock / cold start / RSS", wall)):
        if d:
            tcv = d.get("toolchain", {})
            L.append(f"- {label}: host `{d.get('host')}` ({d.get('platform')}, {d.get('ncpu')} CPUs), "
                     f"node {tcv.get('node_version')}, bun {tcv.get('bun_version')}, run {d.get('started')} → "
                     f"{d.get('finished')}, load-flag threshold {d.get('load_threshold')}.")
    L.append("")
    L.append("Per-iteration cost uses the **two-N method**: every workload is run at two iteration counts n1 < n2 "
             "(same warm-up) and per-iteration = (X(n2) − X(n1)) / (n2 − n1), which cancels process startup, "
             "module init and warm-up. Instructions are `perf stat -e instructions:u` (all threads of the process — "
             "so Node/Bun JIT and GC helper threads count), median of 3 per N. Wall is the median of ≥ 7 "
             "interleaved runs per N. Cold start is the median wall of a `0 0` run (startup + module init only). "
             "Every measured run's stdout is compared byte-for-byte against Node's at the same N.\n")

    ok = [(wid, r) for wid, r in rows if not wid.startswith("control/") and r["perry_node_instr"] is not None]
    ok.sort(key=lambda t: -t[1]["perry_node_instr"])
    L.append("## Workloads ranked by Perry/Node instructions per iteration (slowest first)\n")
    L.append("| # | workload | Perry/Node instr | Perry/Node wall | Bun/Node instr | Bun/Node wall | Node instr/iter | Perry instr/iter | Node µs/iter | Perry µs/iter | first-pass attribution |")
    L.append("|---|---|---|---|---|---|---|---|---|---|---|")
    for i, (wid, r) in enumerate(ok, 1):
        pn, pp = r["arms"].get("node", {}), r["arms"].get("perry", {})
        flag = " ⚠load" if (pp.get("load_flagged_wall_host") or pn.get("load_flagged_wall_host")) else ""
        L.append(f"| {i} | `{wid}` | {fmt_ratio(r['perry_node_instr'])} | {fmt_ratio(r['perry_node_wall'])}{flag} | "
                 f"{fmt_ratio(r['bun_node_instr'])} | {fmt_ratio(r['bun_node_wall'])} | {fmt_instr(pn.get('instr_per_iter'))} | "
                 f"{fmt_instr(pp.get('instr_per_iter'))} | {fmt_us(pn.get('wall_per_iter_s'))} | {fmt_us(pp.get('wall_per_iter_s'))} | "
                 f"{(r.get('note') or '') if (r['perry_node_instr'] or 0) > args.attr_threshold else ''} |")
    wall_only = [(wid, r) for wid, r in rows if not wid.startswith("control/") and r["perry_node_instr"] is None
                 and r["perry_node_wall"] is not None]
    if wall_only:
        L.append("\nWall-only (no instruction ratio):\n")
        for wid, r in wall_only:
            L.append(f"- `{wid}`: Perry/Node wall {fmt_ratio(r['perry_node_wall'])}")
    L.append("\n## Packages (geometric mean over their workloads)\n")
    L.append("| package | workloads | Perry/Node instr | Perry/Node wall | Bun/Node instr | Bun/Node wall |")
    L.append("|---|---|---|---|---|---|")
    pk: dict = {}
    for wid, r in rows:
        if wid.startswith("control/"):
            continue
        pk.setdefault(pkg_of(wid), []).append(r)
    prow = []
    for pkg, rs in pk.items():
        prow.append((pkg, len(rs), geomean([r["perry_node_instr"] for r in rs]), geomean([r["perry_node_wall"] for r in rs]),
                     geomean([r["bun_node_instr"] for r in rs]), geomean([r["bun_node_wall"] for r in rs])))
    prow.sort(key=lambda t: -(t[2] or t[3] or 0))
    for pkg, n, a, b, c, d in prow:
        L.append(f"| {pkg} | {n} | {fmt_ratio(a)} | {fmt_ratio(b)} | {fmt_ratio(c)} | {fmt_ratio(d)} |")

    L.append("\n## Cold start, peak RSS, binary size, compile time\n")
    L.append("| workload | cold node ms | cold bun ms | cold perry ms | RSS node MB | RSS bun MB | RSS perry MB | perry binary MB | perry compile s |")
    L.append("|---|---|---|---|---|---|---|---|---|")

    def ms(x):
        return "—" if x is None else f"{x * 1000:.0f}"

    def mb(kb):
        return "—" if kb is None else f"{kb / 1024:.0f}"

    def size_mb(b):
        return "—" if b is None else f"{b / 1048576:.1f}"
    for wid, r in rows:
        a = r["arms"]
        n, b, p = a.get("node", {}), a.get("bun", {}), a.get("perry", {})
        L.append(f"| `{wid}` | {ms(n.get('cold_s'))} | {ms(b.get('cold_s'))} | {ms(p.get('cold_s'))} | {mb(n.get('peak_rss_kb'))} | "
                 f"{mb(b.get('peak_rss_kb'))} | {mb(p.get('peak_rss_kb'))} | "
                 f"{size_mb(p.get('size_bytes'))} | "
                 f"{'—' if p.get('compile_s') is None else p.get('compile_s')}{' †' if p.get('runtime_rebuilt') else ''} |")
    L.append("\n† this compile was the first with its auto-optimize feature set, so it includes a one-time "
             "runtime+stdlib archive rebuild (cached for later compiles). Compile times were taken on a shared, "
             "loaded host and are indicative only.")
    L.append("\n## Controls (per iteration)\n")
    L.append("| control | node instr | bun instr | perry instr | node µs | bun µs | perry µs |")
    L.append("|---|---|---|---|---|---|---|")
    for wid, r in rows:
        if not wid.startswith("control/"):
            continue
        a = r["arms"]
        L.append(f"| `{wid}` | " + " | ".join(fmt_instr(a.get(x, {}).get("instr_per_iter")) for x in ("node", "bun", "perry"))
                 + " | " + " | ".join(fmt_us(a.get(x, {}).get("wall_per_iter_s")) for x in ("node", "bun", "perry")) + " |")
    L.append("")
    flagged = []
    for wid, r in rows:
        for arm, a in r["arms"].items():
            if a.get("load_flagged_instr_host"):
                flagged.append((wid, arm, a.get("instr_spread"), a.get("load_max_instr_host")))
    if flagged:
        thr = (instr or {}).get("load_threshold")
        worst = max((f[2] or 0) for f in flagged)
        L.append("## Load during the instruction runs\n")
        L.append(f"{len(flagged)} workload×arm instruction measurements ran while the host's 1-minute load exceeded "
                 f"the flag threshold ({thr}); the shared build host was never below it for long. "
                 "`instructions:u` counts only the measured process's user-mode instructions, so load changes "
                 "them far less than wall time — the evidence is the spread of the 3 samples per N, reported "
                 f"per arm in the JSON (`instr_spread` = (max−min)/median over both N). Worst spread among flagged "
                 f"arms: {worst * 100:.1f}%. Arms with spread > 5%:\n")
        big = [f for f in flagged if (f[2] or 0) > 0.05]
        for wid, arm, sp, lm in sorted(big, key=lambda t: -(t[2] or 0)):
            L.append(f"- `{wid}` [{arm}]: spread {sp * 100:.1f}%, load max {lm}")
        if not big:
            L.append("None.")
        L.append("")

    if liv:
        L.append("## Liveness (compiled from source, no removed binding)\n")
        L.append("From a `PERRY_KEEP_SYMBOLS=1` compile of every workload (same commit and flags, symbols kept so the "
                 "binding-symbol check can run). *modules* = natively compiled modules of the package itself, from "
                 "Perry's per-compile SBOM (`node_modules/.cache/perry/audit.json`); *deps* = modules of other "
                 "npm packages pulled in; *JS* = modules routed to a JS runtime (must be 0).\n")
        L.append("| workload | status | modules | deps | JS | binding-symbol check |")
        L.append("|---|---|---|---|---|---|")
        for wid, lr in liv.get("workloads", {}).items():
            lv = lr.get("liveness", {})
            byp = lv.get("modules_by_package", {})
            deps = sum(v for k, v in byp.items() if k not in ("<app>", pkg_of(wid)))
            chk = lv.get("symbol_check", "")
            if lv.get("binding_symbols"):
                chk += "; FOUND " + ", ".join(lv["binding_symbols"][:3])
            L.append(f"| `{wid}` | {lr.get('status')} | {lv.get('package_modules_compiled', '—')} | {deps} | "
                     f"{lv.get('javascript_modules', '—')} | {chk} |")
        L.append("")
    L.append("## MISMATCH / FAIL\n")
    any_bad = False
    skips: dict = {}
    for wid, r in rows:
        for arm, a in r["arms"].items():
            for host in ("instr_host", "wall_host"):
                st = a.get(f"status_{host}")
                reason = (a.get(f"reason_{host}") or "").replace("\n", " ").replace("|", "\\|")
                reason = re.sub(r"\s+", " ", reason)[:300]
                if st == "SKIP":
                    skips.setdefault((host, reason), []).append(wid)
                elif st and st != "OK":
                    any_bad = True
                    L.append(f"- **{st}** `{wid}` [{arm}, {host.replace('_host', '')} host]: {reason}")
    if not any_bad:
        L.append("None.")
    if skips:
        L.append("\n### SKIP (server not available on that host)\n")
        for (host, reason), ws in skips.items():
            L.append(f"- {host.replace('_host', '')} host: {', '.join(f'`{w}`' for w in sorted(set(ws)))} — {reason}")
    L.append("")
    if args.extra and Path(args.extra).exists():
        L.append(Path(args.extra).read_text())
    Path(args.out).write_text("\n".join(L) + "\n")
    log(f"wrote {args.out} and {args.json_out}")


# ---------------------------------------------------------------- main

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    def common(p):
        p.add_argument("--filter", action="append", help="substring of workload id (repeatable); control always runs")
        p.add_argument("--perry-bin-dir", default=os.environ.get("PKG_BENCH_PERRY_BIN_DIR", str(REPO / "target" / "package-bench")))
        p.add_argument("--perry-commit", default=os.environ.get("PKG_BENCH_PERRY_COMMIT", ""))

    pc = sub.add_parser("compile")
    common(pc)
    pc.add_argument("--perry", default=os.environ.get("PKG_BENCH_PERRY", str(REPO / "target" / "release" / "perry")))
    pc.add_argument("--perry-flags", default="", help="extra flags appended to `perry compile` (default none)")
    pc.add_argument("--compile-timeout", type=float, default=1800)
    pc.set_defaults(func=cmd_compile)

    pr = sub.add_parser("run")
    common(pr)
    pr.add_argument("--arms", default="node,bun,perry")
    pr.add_argument("--modes", default="instr", help="comma list of instr,wall,cold,rss (correctness always runs)")
    pr.add_argument("--node", default=default_node())
    pr.add_argument("--bun", default=default_bun())
    pr.add_argument("--allow-version-mismatch", action="store_true")
    pr.add_argument("--out", required=True)
    pr.add_argument("--instr-reps", type=int, default=3)
    pr.add_argument("--wall-reps", type=int, default=7)
    pr.add_argument("--scale", type=float, default=1.0, help="multiply n1/n2 (smoke runs)")
    pr.add_argument("--timeout", type=float, default=600)
    pr.add_argument("--load-threshold", type=float, default=None, help="default 0.5 x ncpu")
    pr.add_argument("--owner", default=os.environ.get("USER", "unknown") + ":package_bench")
    pr.add_argument("--lock-timeout", type=float, default=4 * 3600)
    pr.add_argument("--servers", choices=["auto", "external", "none"], default="auto")
    pr.add_argument("--server-root", default=os.environ.get("PKG_BENCH_SERVER_ROOT", "/tmp/perry-pkg-bench-servers"))
    pr.add_argument("--pg-bin-dir", default=os.environ.get("PKG_BENCH_PG_BIN_DIR"))
    pr.add_argument("--pg-user", default="nobody", help="user to run postgres as when invoked as root")
    pr.add_argument("--pg-port", type=int, default=55432)
    pr.add_argument("--mysqld", default=os.environ.get("PKG_BENCH_MYSQLD"))
    pr.add_argument("--mysql-port", type=int, default=53306)
    pr.add_argument("--mongod", default=os.environ.get("PKG_BENCH_MONGOD"))
    pr.add_argument("--mongo-port", type=int, default=57017)
    pr.add_argument("--redis-server", default=os.environ.get("PKG_BENCH_REDIS_SERVER") or shutil.which("redis-server"))
    pr.add_argument("--redis-port", type=int, default=56379)
    pr.set_defaults(func=cmd_run)

    rp = sub.add_parser("report")
    rp.add_argument("--instr", help="run JSON with instruction counts")
    rp.add_argument("--wall", help="run JSON with wall/cold/rss")
    rp.add_argument("--compile", help="compile.json")
    rp.add_argument("--notes", default=str(PKG_DIR / "attribution.json"))
    rp.add_argument("--extra", help="Markdown appended verbatim (known issues, blockers)")
    rp.add_argument("--liveness", help="compile.json of a PERRY_KEEP_SYMBOLS=1 build (liveness table)")
    rp.add_argument("--profile", help="profile JSON (from `profile`) for the attribution column")
    rp.add_argument("--attr-threshold", type=float, default=2.0,
                    help="only annotate workloads whose Perry/Node instr ratio exceeds this")
    rp.add_argument("--out", default=str(PKG_DIR / "REPORT.md"))
    rp.add_argument("--json-out", default=str(PKG_DIR / "results.json"))
    rp.set_defaults(func=cmd_report)

    pf = sub.add_parser("profile")
    common(pf)
    pf.add_argument("--out", required=True)
    pf.add_argument("--top", type=int, default=5)
    pf.add_argument("--timeout", type=float, default=600)
    pf.add_argument("--include-control", action="store_true")
    for a in pr._actions:
        if a.dest in ("server_root", "pg_bin_dir", "pg_user", "pg_port", "mysqld", "mysql_port", "mongod",
                      "mongo_port", "redis_server", "redis_port", "node"):
            pf._add_action(a)
    pf.set_defaults(func=cmd_profile)

    lk = sub.add_parser("lock")
    lk.add_argument("action", choices=["acquire", "release", "status"])
    lk.add_argument("--owner", default=os.environ.get("USER", "unknown"))

    def lock_cmd(a):
        d = lock_dir()
        if a.action == "status":
            print((d / "owner.json").read_text() if (d / "owner.json").exists() else "free")
        elif a.action == "acquire":
            lock_acquire(a.owner, 4 * 3600)
            atexit.unregister(lock_release)  # a CLI acquire persists until `lock release`
            # Owned by the invoking shell, not this short-lived process, so the
            # stale-lock breaker keeps it while that shell lives.
            info = json.loads((d / "owner.json").read_text())
            info["pid"] = os.getppid()
            (d / "owner.json").write_text(json.dumps(info))
            print(f"acquired {d}")
        else:
            (d / "owner.json").unlink(missing_ok=True)
            d.rmdir()
            print(f"released {d}")
    lk.set_defaults(func=lock_cmd)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
