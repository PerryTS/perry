#!/usr/bin/env python3
"""Dependency footprint of a perry tree: Cargo.lock package count, async/HTTP-stack
crates present, and per-crate reachable dependency counts (cargo tree, normal edges).
usage: deps.py <tree>  -> JSON on stdout"""
import json, re, subprocess, sys, collections
tree = sys.argv[1]
lock = open(tree + "/Cargo.lock").read()
pkgs = re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', lock)
by = collections.defaultdict(list)
for n, v in pkgs: by[n].append(v)
FAMILY = ["tokio", "tokio-util", "tokio-rustls", "tokio-native-tls", "tokio-stream", "tokio-tungstenite", "tokio-macros",
          "hyper", "hyper-util", "hyper-rustls", "hyper-tls", "reqwest", "h2", "tower", "tower-service", "tower-layer",
          "tower-http", "mio", "aws-lc-rs", "aws-lc-sys", "ring", "rustls", "turnloop", "turnloop-io", "quinn", "tungstenite",
          "async-compression", "futures-util", "bytes", "http", "http-body"]
out = {"tree": tree, "lock_packages": len(pkgs), "lock_distinct_names": len(by),
       "family": {n: by[n] for n in FAMILY if n in by},
       "turnloop_like": {n: by[n] for n in by if "turnloop" in n}, "crates": {}}
TOK = re.compile(r"^(tokio[-a-z]*|hyper[-a-z]*|reqwest|h2|mio|tower[-a-z]*|aws-lc-[a-z]+) ")
for p in ["perry", "perry-runtime", "perry-stdlib", "perry-ext-http", "perry-ext-net", "perry-ext-ws", "perry-ext-zlib"]:
    r = subprocess.run(["cargo", "tree", "--offline", "-p", p, "-e", "normal", "--prefix", "none"], cwd=tree, capture_output=True, text=True)
    if r.returncode: out["crates"][p] = {"error": r.stderr[-200:]}; continue
    lines = sorted({l.replace(" (*)", "").strip() for l in r.stdout.splitlines() if l.strip()})
    names = sorted({l.split()[0] for l in lines if not l.startswith("perry") or True})
    out["crates"][p] = {"deps_including_self": len(lines), "async_http_stack": sorted({l.split()[0] + "@" + l.split()[1] for l in lines if TOK.match(l)})}
print(json.dumps(out, indent=1))
